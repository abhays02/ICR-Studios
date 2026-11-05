//! LineNet output decoding: `det_post.decode`, plus the box-merging rules the
//! multi-scale sweep needs.
//!
//! The network emits `[3, h, w]` at half the input resolution: a core-channel
//! logit, and the distance from each core pixel up to the top and down to the
//! bottom of its line. Boxes are recovered by thresholding the core, taking
//! 4-connected components, and averaging the distances within each.

use crate::params::{DetectParams, CORE_X_SHRINK, OUT_STRIDE};

/// A detected text line in input pixels, before projection back to the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Mean core probability over the component.
    pub score: f32,
}

impl LineBox {
    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }

    pub fn height(&self) -> f32 {
        (self.y1 - self.y0).max(1.0)
    }

    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    /// Intersection over union.
    pub fn iou(&self, other: &Self) -> f32 {
        let ox = (self.x1.min(other.x1) - self.x0.max(other.x0)).max(0.0);
        let oy = (self.y1.min(other.y1) - self.y0.max(other.y0)).max(0.0);
        let inter = ox * oy;
        if inter <= 0.0 {
            return 0.0;
        }
        let union = self.area() + other.area() - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }

    /// Vertical overlap as a fraction of the shorter box's height. Two fragments
    /// of one handwritten line share almost all of it.
    pub fn vertical_overlap(&self, other: &Self) -> f32 {
        let oy = (self.y1.min(other.y1) - self.y0.max(other.y0)).max(0.0);
        let shorter = self.height().min(other.height());
        if shorter <= 0.0 {
            0.0
        } else {
            oy / shorter
        }
    }
}

#[inline]
fn sigmoid(x: f32) -> f32 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}

/// `cv2.connectedComponentsWithStats(mask, connectivity=4)`, reduced to the two
/// statistics `decode` uses: area and the leftmost column plus run width.
///
/// Iterative flood fill with an explicit stack — a scanned page can hold a
/// component spanning tens of thousands of pixels, which would blow the native
/// stack and is fatal in WASM where the stack is small and fixed.
struct Components {
    labels: Vec<i32>,
    width: usize,
    height: usize,
    /// Per label: (area, left, right_exclusive).
    stats: Vec<(usize, usize, usize)>,
}

impl Components {
    fn new(width: usize, height: usize) -> Self {
        Self {
            labels: vec![0; width * height],
            width,
            height,
            stats: vec![(0, 0, 0)], // index 0 is background
        }
    }

    fn run(mask: &[bool], width: usize, height: usize) -> Self {
        let mut c = Self::new(width, height);
        let mut stack: Vec<usize> = Vec::with_capacity(1024);
        let mut label = 0i32;
        for start in 0..width * height {
            if !mask[start] || c.labels[start] != 0 {
                continue;
            }
            label += 1;
            let lid = label as usize;
            c.stats.push((0, usize::MAX, 0));
            stack.clear();
            stack.push(start);
            c.labels[start] = label;
            while let Some(p) = stack.pop() {
                let y = p / width;
                let x = p % width;
                let s = &mut c.stats[lid];
                s.0 += 1;
                if x < s.1 {
                    s.1 = x;
                }
                if x + 1 > s.2 {
                    s.2 = x + 1;
                }
                // 4-connectivity, matching cv2's default for this call.
                if x > 0 && mask[p - 1] && c.labels[p - 1] == 0 {
                    c.labels[p - 1] = label;
                    stack.push(p - 1);
                }
                if x + 1 < width && mask[p + 1] && c.labels[p + 1] == 0 {
                    c.labels[p + 1] = label;
                    stack.push(p + 1);
                }
                if y > 0 && mask[p - width] && c.labels[p - width] == 0 {
                    c.labels[p - width] = label;
                    stack.push(p - width);
                }
                if y + 1 < height && mask[p + width] && c.labels[p + width] == 0 {
                    c.labels[p + width] = label;
                    stack.push(p + width);
                }
            }
        }
        c
    }

    fn count(&self) -> usize {
        self.stats.len()
    }
}

/// `det_post.decode`.
///
/// `out` is the detector's `[3, h, w]` output, row-major, channel-major:
/// channel 0 is the core logit, 1 the distance to the line top, 2 to the
/// bottom. Returns boxes in *input* pixels (the caller divides by the page
/// scale to get original pixels).
pub fn decode(out: &[f32], out_h: usize, out_w: usize, p: &DetectParams) -> Vec<LineBox> {
    if out.len() < 3 * out_h * out_w || out_h == 0 || out_w == 0 {
        return Vec::new();
    }
    let plane = out_h * out_w;
    let core = &out[0..plane];
    let dtop = &out[plane..2 * plane];
    let dbot = &out[2 * plane..3 * plane];

    let mut prob = vec![0f32; plane];
    let mut mask = vec![false; plane];
    for i in 0..plane {
        let s = sigmoid(core[i]);
        prob[i] = s;
        mask[i] = s > p.thresh;
    }

    let comps = Components::run(&mask, out_w, out_h);
    if comps.count() <= 1 {
        return Vec::new();
    }

    let n = comps.count();
    let mut sum_top = vec![0f64; n];
    let mut sum_bot = vec![0f64; n];
    let mut sum_prob = vec![0f64; n];
    let mut cnt = vec![0f64; n];

    let stride = OUT_STRIDE as f64;
    for y in 0..out_h {
        // Pixel centre in input pixels, exactly as `encode_targets` placed it.
        let yc = (y as f64 + 0.5) * stride;
        let row = y * out_w;
        for x in 0..out_w {
            let i = row + x;
            let l = comps.labels[i];
            if l <= 0 {
                continue;
            }
            let li = l as usize;
            sum_top[li] += (yc - dtop[i] as f64) as f64;
            sum_bot[li] += (yc + dbot[i] as f64) as f64;
            sum_prob[li] += prob[i] as f64;
            cnt[li] += 1.0;
        }
    }

    let mut boxes = Vec::new();
    for li in 1..n {
        let (area, left, right) = comps.stats[li];
        if area < p.min_area {
            continue;
        }
        let c = if cnt[li] > 0.0 { cnt[li] } else { 1.0 };
        let y0 = sum_top[li] / c;
        let y1 = sum_bot[li] / c;
        let score = sum_prob[li] / c;
        let lh = (y1 - y0).max(1.0) as f32;
        // The core is narrower than the line by CORE_X_SHRINK at each end, so
        // put that width back. Capped so a very tall fragment cannot explode
        // sideways.
        let ex = (CORE_X_SHRINK * lh).min(0.5 * lh);
        let x0 = left as f32 * OUT_STRIDE as f32 - ex;
        let x1 = right as f32 * OUT_STRIDE as f32 + ex;
        boxes.push(LineBox {
            x0: x0 - p.grow_x * lh,
            y0: (y0 - p.grow_y as f64 * lh as f64) as f32,
            x1: x1 + p.grow_x * lh,
            y1: (y1 + p.grow_y as f64 * lh as f64) as f32,
            score: score as f32,
        });
    }
    // Reading order is assigned later, but a stable sort here keeps results
    // deterministic across runs and platforms.
    boxes.sort_by(|a, b| {
        a.y0
            .partial_cmp(&b.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
    });
    boxes
}

/// Merge boxes the detector split across one physical line.
///
/// Handwriting is the reason this exists: a cursive stroke that lifts off the
/// baseline, or a descender that dips below the core band, produces two
/// components whose vertical extents almost entirely overlap. Merging on
/// vertical overlap alone would glue separate columns together, so the boxes
/// must also be close horizontally.
pub fn merge_fragments(boxes: &mut Vec<LineBox>, v_overlap: f32, h_gap_line_heights: f32) {
    if boxes.len() < 2 {
        return;
    }
    boxes.sort_by(|a, b| {
        a.y0
            .partial_cmp(&b.y0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut merged: Vec<LineBox> = Vec::with_capacity(boxes.len());
    for b in boxes.drain(..) {
        let mut placed = false;
        for m in merged.iter_mut() {
            if m.vertical_overlap(&b) < v_overlap {
                continue;
            }
            let gap = (b.x0 - m.x1).max(m.x0 - b.x1).max(0.0);
            if gap > h_gap_line_heights * m.height() {
                continue;
            }
            // Area-weighted score so a large confident line is not dragged down
            // by a small noisy neighbour.
            let wa = m.area();
            let wb = b.area();
            let wsum = (wa + wb).max(1e-6);
            m.x0 = m.x0.min(b.x0);
            m.y0 = (m.y0 * wa + b.y0 * wb) / wsum;
            m.x1 = m.x1.max(b.x1);
            m.y1 = (m.y1 * wa + b.y1 * wb) / wsum;
            m.score = (m.score * wa + b.score * wb) / wsum;
            placed = true;
            break;
        }
        if !placed {
            merged.push(b);
        }
    }
    *boxes = merged;
}

/// Drop boxes that are contained within a larger, more confident one. Produced
/// by the multi-scale sweep, where two passes see the same line at different
/// resolutions.
pub fn dedupe_contained(boxes: &mut Vec<LineBox>, iou_thresh: f32) {
    let n = boxes.len();
    if n < 2 {
        return;
    }
    let mut drop = vec![false; n];
    for i in 0..n {
        if drop[i] {
            continue;
        }
        for j in (i + 1)..n {
            if drop[j] {
                continue;
            }
            if boxes[i].iou(&boxes[j]) >= iou_thresh {
                // Keep the larger area; it covers more of the physical line.
                if boxes[i].area() >= boxes[j].area() {
                    drop[j] = true;
                } else {
                    drop[i] = true;
                    break;
                }
            }
        }
    }
    let mut i = 0;
    boxes.retain(|_| {
        let keep = !drop[i];
        i += 1;
        keep
    });
}

/// Clip boxes to the page and discard degenerate ones.
pub fn clip_to_page(boxes: &mut Vec<LineBox>, page_w: usize, page_h: usize) {
    let w = page_w as f32;
    let h = page_h as f32;
    for b in boxes.iter_mut() {
        b.x0 = b.x0.clamp(0.0, w);
        b.x1 = b.x1.clamp(0.0, w);
        b.y0 = b.y0.clamp(0.0, h);
        b.y1 = b.y1.clamp(0.0, h);
    }
    boxes.retain(|b| b.x1 - b.x0 >= 2.0 && b.y1 - b.y0 >= 2.0);
}

/// Median line height, the statistic the adaptive re-detect rule uses.
pub fn median_height(boxes: &[LineBox]) -> f32 {
    if boxes.is_empty() {
        return 0.0;
    }
    let mut hs: Vec<f32> = boxes.iter().map(|b| b.y1 - b.y0).collect();
    hs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = hs.len();
    if n % 2 == 1 {
        hs[n / 2]
    } else {
        (hs[n / 2 - 1] + hs[n / 2]) / 2.0
    }
}

/// Scale input-pixel boxes back to original page pixels.
pub fn unscale(boxes: &mut [LineBox], scale: f64) {
    let s = scale as f32;
    if s <= 0.0 {
        return;
    }
    for b in boxes.iter_mut() {
        b.x0 /= s;
        b.y0 /= s;
        b.x1 /= s;
        b.y1 /= s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> DetectParams {
        DetectParams::indra()
    }

    /// Build a synthetic [3,h,w] detector output with one horizontal core band.
    fn synthetic_line(h: usize, w: usize, y0: usize, y1: usize, x0: usize, x1: usize) -> Vec<f32> {
        let mut out = vec![0f32; 3 * h * w];
        let plane = h * w;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * w + x;
                out[i] = 6.0; // sigmoid(6) ~ 0.9975, comfortably over any threshold
                // Distances from this row to the top and bottom of the line.
                out[plane + i] = ((y - y0) as f32 + 0.5) * OUT_STRIDE as f32;
                out[2 * plane + i] = ((y1 - 1 - y) as f32 + 0.5) * OUT_STRIDE as f32;
            }
        }
        out
    }

    #[test]
    fn empty_map_yields_no_boxes() {
        let out = vec![0f32; 3 * 8 * 8];
        assert!(decode(&out, 8, 8, &params()).is_empty());
    }

    #[test]
    fn single_band_becomes_one_box() {
        let (h, w) = (16, 32);
        let out = synthetic_line(h, w, 6, 9, 4, 20);
        let boxes = decode(&out, h, w, &params());
        assert_eq!(boxes.len(), 1);
        let b = boxes[0];
        assert!(b.score > 0.9, "score {}", b.score);
        // x range covers the core, widened by CORE_X_SHRINK of the line height.
        assert!(b.x0 < 4.0 * OUT_STRIDE as f32);
        assert!(b.x1 > 20.0 * OUT_STRIDE as f32);
        assert!(b.y1 > b.y0);
    }

    #[test]
    fn a_low_threshold_on_unconfident_logits_merges_the_page() {
        // The hazard `DetectParams::handwriting` documents, reproduced: with
        // background logits at 0.0 the background probability is sigmoid(0)=0.5,
        // so any threshold below 0.5 makes the entire map one component and two
        // lines collapse into one meaningless box.
        let (h, w) = (16, 32);
        let plane = h * w;
        let mut out = vec![0f32; 3 * plane];
        for &(y0, y1) in &[(3, 6), (10, 13)] {
            for y in y0..y1 {
                for x in 4..20 {
                    let i = y * w + x;
                    out[i] = 5.0;
                    out[plane + i] = ((y - y0) as f32 + 0.5) * 2.0;
                    out[2 * plane + i] = ((y1 - 1 - y) as f32 + 0.5) * 2.0;
                }
            }
        }
        assert_eq!(decode(&out, h, w, &params()).len(), 2, "two lines at the reference threshold");
        let flooded = DetectParams { thresh: 0.3, ..params() };
        assert_eq!(
            decode(&out, h, w, &flooded).len(),
            1,
            "threshold 0.3 floods the background and merges both lines into one box"
        );
    }

    #[test]
    fn handwriting_profile_keeps_the_reference_threshold() {
        assert_eq!(DetectParams::handwriting().thresh, 0.5);
        assert_eq!(DetectParams::handwriting().min_area, 1);
        assert!(DetectParams::handwriting().grow_y > 0.0);
    }

    #[test]
    fn min_area_gates_single_pixel_components() {
        let (h, w) = (8, 8);
        let plane = h * w;
        let mut out = vec![0f32; 3 * plane];
        let i = 3 * w + 3;
        out[i] = 6.0;
        out[plane + i] = 1.0;
        out[2 * plane + i] = 1.0;
        // area 1: reference min_area 2 drops it, handwriting min_area 1 keeps it.
        assert!(decode(&out, h, w, &params()).is_empty());
        assert_eq!(decode(&out, h, w, &DetectParams::handwriting()).len(), 1);
    }

    #[test]
    fn grow_widens_boxes() {
        let (h, w) = (16, 32);
        let out = synthetic_line(h, w, 6, 9, 4, 20);
        let base = decode(&out, h, w, &params());
        let grown = decode(
            &out,
            h,
            w,
            &DetectParams {
                grow_y: 0.5,
                grow_x: 0.5,
                ..params()
            },
        );
        assert!(grown[0].y0 < base[0].y0);
        assert!(grown[0].y1 > base[0].y1);
        assert!(grown[0].x0 < base[0].x0);
        assert!(grown[0].x1 > base[0].x1);
    }

    #[test]
    fn merge_joins_two_fragments_of_one_line() {
        let mut boxes = vec![
            LineBox { x0: 10.0, y0: 100.0, x1: 60.0, y1: 118.0, score: 0.9 },
            LineBox { x0: 62.0, y0: 101.0, x1: 120.0, y1: 119.0, score: 0.8 },
        ];
        merge_fragments(&mut boxes, 0.7, 0.6);
        assert_eq!(boxes.len(), 1);
        assert!(boxes[0].x0 <= 10.0 && boxes[0].x1 >= 120.0);
    }

    #[test]
    fn merge_keeps_separate_lines_apart() {
        let mut boxes = vec![
            LineBox { x0: 10.0, y0: 100.0, x1: 200.0, y1: 118.0, score: 0.9 },
            LineBox { x0: 10.0, y0: 140.0, x1: 200.0, y1: 158.0, score: 0.9 },
        ];
        merge_fragments(&mut boxes, 0.7, 0.6);
        assert_eq!(boxes.len(), 2, "distinct lines must not merge");
    }

    #[test]
    fn dedupe_removes_contained_box() {
        let mut boxes = vec![
            LineBox { x0: 10.0, y0: 100.0, x1: 200.0, y1: 120.0, score: 0.9 },
            LineBox { x0: 12.0, y0: 102.0, x1: 198.0, y1: 118.0, score: 0.7 },
        ];
        dedupe_contained(&mut boxes, 0.6);
        assert_eq!(boxes.len(), 1);
        assert!(boxes[0].score > 0.85, "the larger, more confident box survives");
    }

    #[test]
    fn median_height_matches_numpy() {
        let boxes = vec![
            LineBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 10.0, score: 1.0 },
            LineBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 20.0, score: 1.0 },
            LineBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 30.0, score: 1.0 },
        ];
        assert!((median_height(&boxes) - 20.0).abs() < 1e-6);
        assert!((median_height(&boxes[..2]) - 15.0).abs() < 1e-6);
        assert_eq!(median_height(&[]), 0.0);
    }

    #[test]
    fn clip_discards_degenerate_boxes() {
        let mut boxes = vec![
            LineBox { x0: -5.0, y0: -5.0, x1: 300.0, y1: 400.0, score: 0.9 },
            LineBox { x0: 10.0, y0: 10.0, x1: 10.5, y1: 50.0, score: 0.9 },
        ];
        clip_to_page(&mut boxes, 200, 300);
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].x1, 200.0);
        assert_eq!(boxes[0].y1, 300.0);
    }

    #[test]
    fn iou_is_symmetric_and_bounded() {
        let a = LineBox { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0, score: 1.0 };
        let b = LineBox { x0: 5.0, y0: 5.0, x1: 15.0, y1: 15.0, score: 1.0 };
        let i = a.iou(&b);
        assert!((i - b.iou(&a)).abs() < 1e-6);
        assert!((i - 25.0 / 175.0).abs() < 1e-4);
        assert_eq!(a.iou(&a), 1.0);
    }
}
