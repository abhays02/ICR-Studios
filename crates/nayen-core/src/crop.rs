//! Line cropping and batching: `rec_preproc.crop_line`, `is_dark_line`,
//! `pad_batch` and `width_batches`.
//!
//! The recognizers read dark ink on light paper at a fixed text scale, so the
//! crop rule is not cosmetic. Training and inference must agree on it exactly,
//! which is why every constant here is reproduced from `rec_preproc.py` rather
//! than re-derived.

use crate::det::LineBox;
use crate::image::{percentiles, Gray8, InkMap};
use crate::params::{CropParams, MAX_BATCH, MAX_BATCH_AREA, MAX_WIDTH_RATIO, MIN_BATCH_WIDTH, WIDTH_MULTIPLE};
use crate::tensor::{NamedTensor, Tensor};

/// Light text on a dark ground (a banner, a dark table head, a slide). The
/// recognizers only read dark-on-light, so such a line is read inverted.
///
/// The test is asymmetric tails around the median: if the bright tail is more
/// than twice the dark tail (plus a floor to ignore noise), the *ground* is
/// dark and the text is the bright minority.
pub fn is_dark_line(gray: &Gray8, b: &LineBox) -> bool {
    let (h, w) = (gray.height as i64, gray.width as i64);
    let x0 = (b.x0.floor() as i64).clamp(0, w);
    let x1 = ((b.x1.floor() as i64) + 1).clamp(0, w);
    let y0 = (b.y0.floor() as i64).clamp(0, h);
    let y1 = ((b.y1.floor() as i64) + 1).clamp(0, h);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let mut pixels = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
    for y in y0..y1 {
        let row = y as usize * gray.width;
        for x in x0..x1 {
            pixels.push(gray.data[row + x as usize]);
        }
    }
    let p = percentiles(&pixels, &[0.02, 0.5, 0.98]);
    let (lo, med, hi) = (p[0], p[1], p[2]);
    hi - med > 2.0 * (med - lo) + 8.0
}

/// A cropped, resized line ready to be batched. `ink` is `[height, width]`.
#[derive(Clone, Debug, PartialEq)]
pub struct LineCrop {
    pub height: usize,
    pub width: usize,
    pub ink: Vec<f32>,
    pub dark: bool,
}

impl LineCrop {
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.ink[y * self.width + x]
    }
}

/// Stretch a crop to the full 0..=1 range using its 2nd and 98th percentiles.
///
/// Faint pencil occupies a narrow band near white; the recognizer was trained
/// on ink that spans most of the range, so without this the strokes are
/// effectively invisible to it. Percentiles rather than min/max, because a
/// single speck of scanner noise would otherwise define the whole mapping.
fn normalize_contrast(crop: &mut [u8]) {
    if crop.len() < 16 {
        return;
    }
    let p = percentiles(crop, &[0.02, 0.98]);
    let (lo, hi) = (p[0], p[1]);
    let range = hi - lo;
    if range < 8.0 {
        return; // Flat region: stretching would amplify pure noise.
    }
    for v in crop.iter_mut() {
        let n = ((*v as f64 - lo) / range * 255.0).round();
        *v = n.clamp(0.0, 255.0) as u8;
    }
}

/// `rec_preproc.crop_line`.
///
/// Expands the box by `pad_y`/`pad_x` of its own height, crops that region
/// (paper outside the page), and resizes to `LINE_HEIGHT * window` tall while
/// preserving aspect ratio. Returns an ink map: 0 = paper, 1 = ink.
pub fn crop_line(gray: &Gray8, b: &LineBox, p: &CropParams) -> Result<LineCrop, CropError> {
    let pad_y = p.effective_pad_y();
    let height = p.effective_height();

    let h = ((b.y1 - b.y0).max(1.0)) as f64;
    let x0f = b.x0 as f64 - p.pad_x * h;
    let x1f = b.x1 as f64 + p.pad_x * h;
    let y0f = b.y0 as f64 - pad_y * h;
    let y1f = b.y1 as f64 + pad_y * h;

    let ix0 = x0f.floor() as i64;
    let iy0 = y0f.floor() as i64;
    let ix1 = x1f.ceil() as i64;
    let iy1 = y1f.ceil() as i64;
    let cw = (ix1 - ix0).max(1) as usize;
    let ch = (iy1 - iy0).max(1) as usize;

    let dark = is_dark_line(gray, b);
    let fill: u8 = if dark { 0 } else { 255 };
    let mut crop = vec![fill; cw * ch];

    // Clamp the source rect to the page; anything outside keeps `fill`, which is
    // how the reference pads off-page margins.
    let sx0 = ix0.max(0) as usize;
    let sy0 = iy0.max(0) as usize;
    let sx1 = (ix1 as usize).min(gray.width);
    let sy1 = (iy1 as usize).min(gray.height);
    if sx1 > sx0 && sy1 > sy0 {
        for y in sy0..sy1 {
            let dst_row = ((y as i64 - iy0) as usize) * cw;
            let src_row = y * gray.width;
            let dst_off = (sx0 as i64 - ix0) as usize;
            let n = sx1 - sx0;
            crop[dst_row + dst_off..dst_row + dst_off + n]
                .copy_from_slice(&gray.data[src_row + sx0..src_row + sx1]);
        }
    }

    if p.normalize_contrast {
        normalize_contrast(&mut crop);
    }

    let src = Gray8::new(cw, ch, crop)?;
    // Width follows from the aspect ratio, clamped to a multiple of the
    // timestep stride and the recognizer's maximum.
    let out_w = ((cw as f64 * height as f64 / ch as f64).round() as usize)
        .max(WIDTH_MULTIPLE)
        .min(p.max_width);
    let resized = src.resize(out_w, height)?;

    let ink: Vec<f32> = if dark {
        // Already light-on-dark: the ink IS the bright pixels.
        resized.data.iter().map(|&v| v as f32 / 255.0).collect()
    } else {
        resized.data.iter().map(|&v| 1.0 - v as f32 / 255.0).collect()
    };

    Ok(LineCrop {
        height,
        width: out_w,
        ink,
        dark,
    })
}

/// `rec_preproc.pad_batch`: right-pad a set of crops to one width (rounded up
/// to `WIDTH_MULTIPLE`) and return the `[B,1,H,W]` tensor plus the `[B]` int64
/// real widths the model uses to ignore padding.
pub fn pad_batch(crops: &[&LineCrop]) -> Option<(NamedTensor, NamedTensor)> {
    if crops.is_empty() {
        return None;
    }
    let h = crops[0].height;
    let max_w = crops.iter().map(|c| c.width).max()?;
    let wm = max_w.div_ceil(WIDTH_MULTIPLE) * WIDTH_MULTIPLE;
    let b = crops.len();

    let mut data = vec![0f32; b * h * wm];
    let mut widths = vec![0i64; b];
    for (i, c) in crops.iter().enumerate() {
        for y in 0..h {
            let dst = i * h * wm + y * wm;
            let src = y * c.width;
            data[dst..dst + c.width].copy_from_slice(&c.ink[src..src + c.width]);
        }
        widths[i] = c.width as i64;
    }
    Some((
        NamedTensor::new("x", Tensor::f32(vec![b as i64, 1, h as i64, wm as i64], data)),
        NamedTensor::new("widths", Tensor::i64(vec![b as i64], widths)),
    ))
}

/// `rec_preproc.width_batches`: group line indices into batches of similar
/// width so little compute is spent on padding.
///
/// Narrowest first. A batch closes at `MAX_BATCH` lines, when the next line is
/// wider than `MAX_WIDTH_RATIO` times the batch's first line, or when
/// lines x width would exceed `MAX_BATCH_AREA`. That last bound is a real
/// memory guard: ONNX Runtime keeps its largest allocation, and 64 lines of
/// 4096px held 5GB on large scanned pages.
pub fn width_batches(widths: &[usize]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..widths.len()).collect();
    order.sort_by_key(|&i| widths[i]);

    let mut batches: Vec<Vec<usize>> = Vec::new();
    for i in order {
        let wi = widths[i].max(MIN_BATCH_WIDTH);
        let fits = batches
            .last()
            .map(|b| {
                let first = widths[b[0]].max(MIN_BATCH_WIDTH);
                b.len() < MAX_BATCH
                    && (wi as f32) <= MAX_WIDTH_RATIO * (first as f32)
                    && (b.len() + 1) * wi <= MAX_BATCH_AREA
            })
            .unwrap_or(false);
        if fits {
            batches.last_mut().unwrap().push(i);
        } else {
            batches.push(vec![i]);
        }
    }
    batches
}

#[derive(Clone, Debug, PartialEq)]
pub enum CropError {
    Image(String),
}

impl std::fmt::Display for CropError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image(e) => write!(f, "crop failed: {e}"),
        }
    }
}

impl std::error::Error for CropError {}

impl From<crate::image::ImageError> for CropError {
    fn from(e: crate::image::ImageError) -> Self {
        Self::Image(e.to_string())
    }
}

/// Build an [`InkMap`] view of a crop, for hosts that want to draw it.
impl From<&LineCrop> for InkMap {
    fn from(c: &LineCrop) -> Self {
        InkMap {
            width: c.width,
            height: c.height,
            data: c.ink.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of light paper with one line of sparse dark glyph strokes.
    ///
    /// Sparse, with the strokes inset from the box, so the box holds paper as
    /// well as ink. A solid bar with no margin is not what the detector emits,
    /// and it flips `is_dark_line`: the reference flags a box whose bright tail
    /// is longer than its dark tail, which a solid dark bar technically is.
    fn page_with_line(w: usize, h: usize, y0: usize, y1: usize) -> Gray8 {
        let mut data = vec![250u8; w * h];
        let mut x = 24usize;
        while x < w - 24 {
            for y in (y0 + 3)..(y1 - 3) {
                for xx in x..(x + 4).min(w) {
                    data[y * w + xx] = 25;
                }
            }
            x += 7;
        }
        Gray8::new(w, h, data).unwrap()
    }

    /// A dark banner with thin light glyphs: the light-on-dark case.
    fn banner_page(w: usize, h: usize) -> Gray8 {
        let mut data = vec![250u8; w * h];
        for y in 30..90.min(h) {
            for x in 0..w {
                data[y * w + x] = 28;
            }
        }
        let mut x = 40usize;
        while x < 360.min(w) {
            for y in 45..75.min(h) {
                for xx in x..(x + 4) {
                    data[y * w + xx] = 235;
                }
            }
            x += 9;
        }
        Gray8::new(w, h, data).unwrap()
    }

    fn mean_ink(c: &LineCrop) -> f64 {
        c.ink.iter().map(|&v| v as f64).sum::<f64>() / c.ink.len() as f64
    }

    #[test]
    fn crop_height_is_line_height_times_window() {
        let page = page_with_line(400, 200, 80, 100);
        let b = LineBox { x0: 10.0, y0: 80.0, x1: 390.0, y1: 100.0, score: 0.9 };
        let c = crop_line(&page, &b, &CropParams::indra()).unwrap();
        assert_eq!(c.height, 96, "rec.onnx expects [B,1,96,W]");
        // The width is NOT rounded here; pad_batch does that, matching the
        // reference where crop_line keeps the aspect-ratio width exactly.
        assert!(c.width >= WIDTH_MULTIPLE);
        assert!(!c.dark, "dark glyphs on light paper must not be inverted");
    }

    #[test]
    fn script_classifier_crop_is_thirty_two() {
        let page = page_with_line(400, 200, 80, 100);
        let b = LineBox { x0: 10.0, y0: 80.0, x1: 390.0, y1: 100.0, score: 0.9 };
        let c = crop_line(&page, &b, &CropParams::script_classifier()).unwrap();
        assert_eq!(c.height, 32, "script.onnx expects [B,1,32,W]");
    }

    #[test]
    fn ink_map_concentrates_where_the_glyphs_are() {
        let page = page_with_line(400, 200, 80, 100);
        let b = LineBox { x0: 20.0, y0: 78.0, x1: 380.0, y1: 102.0, score: 0.9 };
        let c = crop_line(&page, &b, &CropParams::indra()).unwrap();
        assert!(c.ink.iter().all(|&v| (0.0..=1.0).contains(&v)), "ink is normalized");

        let row_mean = |y: usize| {
            c.ink[y * c.width..(y + 1) * c.width]
                .iter()
                .map(|&v| v as f64)
                .sum::<f64>()
                / c.width as f64
        };
        // The window=3 crop puts the glyphs in the middle band and paper above
        // and below, so the middle must be far inkier than the margins.
        let mid = row_mean(c.height / 2);
        let top = row_mean(1);
        let bottom = row_mean(c.height - 2);
        assert!(mid > top * 4.0, "middle band {mid} vs top margin {top}");
        assert!(mid > bottom * 4.0, "middle band {mid} vs bottom margin {bottom}");
        assert!(mid > 0.2, "glyph band should be substantially ink, got {mid}");
    }

    #[test]
    fn max_ink_matches_the_darkest_glyph() {
        let page = page_with_line(400, 200, 80, 100);
        let b = LineBox { x0: 20.0, y0: 78.0, x1: 380.0, y1: 102.0, score: 0.9 };
        let c = crop_line(&page, &b, &CropParams::indra()).unwrap();
        let max = c.ink.iter().cloned().fold(f32::MIN, f32::max);
        // 1 - 25/255: the glyph colour, undiluted by resampling.
        assert!((max - (1.0 - 25.0 / 255.0)).abs() < 1e-5, "max ink {max}");
    }

    #[test]
    fn more_vertical_padding_shrinks_the_glyph_scale() {
        // Counter-intuitive and worth pinning: the output height is fixed at
        // LINE_HEIGHT * window, so a taller source region (larger pad_y) means
        // the same glyphs are resampled *smaller*, and the crop comes out
        // narrower. Reference widths on the golden page: 328 at pad_y 0.2,
        // 274 at pad_y 0.35.
        let page = page_with_line(400, 200, 80, 100);
        let b = LineBox { x0: 20.0, y0: 78.0, x1: 380.0, y1: 102.0, score: 0.9 };
        let tight = crop_line(&page, &b, &CropParams::indra()).unwrap();
        let loose = crop_line(&page, &b, &CropParams::handwriting()).unwrap();
        assert_eq!(loose.height, tight.height, "output height is fixed");
        assert!(
            loose.width < tight.width,
            "pad_y 0.35 shrinks glyph scale: {} vs {} wide",
            loose.width,
            tight.width
        );
        // Larger pad_x alone still widens the crop.
        let wide_x = crop_line(
            &page,
            &b,
            &CropParams {
                pad_x: 0.6,
                ..CropParams::indra()
            },
        )
        .unwrap();
        assert!(wide_x.width > tight.width, "pad_x 0.6 > 0.3 must widen the crop");
    }

    #[test]
    fn box_off_page_edge_is_padded_not_clipped() {
        let page = page_with_line(400, 200, 80, 100);
        // Line runs off the left edge; the crop must still be full height.
        let b = LineBox { x0: -30.0, y0: 80.0, x1: 100.0, y1: 100.0, score: 0.9 };
        let c = crop_line(&page, &b, &CropParams::indra()).unwrap();
        assert_eq!(c.height, 96);
        assert!(c.width > 0);
    }

    #[test]
    fn dark_line_is_detected_and_inverted() {
        // A dark field with thin light glyphs. The box must contain the dark
        // ground, not just the glyphs: that is what makes the median dark and
        // the bright tail the longer one.
        let page = banner_page(400, 200);
        let b = LineBox { x0: 20.0, y0: 30.0, x1: 380.0, y1: 90.0, score: 0.9 };
        assert!(is_dark_line(&page, &b), "light glyphs on a dark field must read as a dark line");
        let c = crop_line(&page, &b, &CropParams::indra()).unwrap();
        assert!(c.dark);
        // Inverted, so the glyphs are still ink=1 for the recognizer. Reference
        // mean on the golden banner is 0.551.
        assert!(mean_ink(&c) > 0.3, "inverted banner ink mean {}", mean_ink(&c));

        // The same page read as an ordinary line box (dark ground, no light
        // majority) must not be flagged.
        let normal = page_with_line(400, 200, 80, 100);
        let nb = LineBox { x0: 20.0, y0: 78.0, x1: 380.0, y1: 102.0, score: 0.9 };
        assert!(!is_dark_line(&normal, &nb), "dark ink on paper is not a dark line");
    }

    #[test]
    fn pad_batch_shape_and_widths() {
        let page = page_with_line(400, 200, 80, 100);
        let b1 = LineBox { x0: 10.0, y0: 80.0, x1: 390.0, y1: 100.0, score: 0.9 };
        let b2 = LineBox { x0: 10.0, y0: 80.0, x1: 120.0, y1: 100.0, score: 0.9 };
        let c1 = crop_line(&page, &b1, &CropParams::indra()).unwrap();
        let c2 = crop_line(&page, &b2, &CropParams::indra()).unwrap();
        let (x, widths) = pad_batch(&[&c1, &c2]).unwrap();
        let expected = (c1.width.max(c2.width)).div_ceil(WIDTH_MULTIPLE) * WIDTH_MULTIPLE;
        assert_eq!(x.tensor.shape, vec![2, 1, 96, expected as i64], "padded up to a multiple of 8");
        assert_eq!(x.tensor.shape[3] % WIDTH_MULTIPLE as i64, 0);
        let w = widths.tensor.data.as_i64().unwrap();
        assert_eq!(w, &[c1.width as i64, c2.width as i64]);
        // Padding columns must be paper, not garbage.
        let wm = x.tensor.shape[3] as usize;
        let data = x.tensor.data.as_f32().unwrap();
        assert!(c2.width < wm, "the narrow crop must actually be padded");
        assert!(
            data[1 * 96 * wm + c2.width..1 * 96 * wm + wm].iter().all(|&v| v == 0.0),
            "every padding column must be paper"
        );
    }

    #[test]
    fn pad_batch_of_nothing_is_none() {
        assert!(pad_batch(&[]).is_none());
    }

    #[test]
    fn width_batches_groups_similar_widths() {
        let widths = vec![100, 102, 98, 101, 900, 910];
        let b = width_batches(&widths);
        // The four narrow ones share a batch; the two wide ones another.
        assert_eq!(b.len(), 2, "{b:?}");
        assert_eq!(b[0].len(), 4);
        assert_eq!(b[1].len(), 2);
    }

    #[test]
    fn width_batches_respects_max_batch() {
        let widths = vec![64; MAX_BATCH * 2 + 5];
        let b = width_batches(&widths);
        assert!(b.iter().all(|x| x.len() <= MAX_BATCH));
        assert_eq!(b.iter().map(|x| x.len()).sum::<usize>(), widths.len());
    }

    #[test]
    fn width_batches_caps_area() {
        // 64 lines of 4096px is exactly the 5GB case the bound exists for.
        let widths = vec![4096; MAX_BATCH];
        let b = width_batches(&widths);
        for batch in &b {
            assert!(batch.len() * 4096 <= MAX_BATCH_AREA, "batch too large: {}", batch.len());
        }
    }

    #[test]
    fn contrast_normalization_stretches_faint_pencil() {
        // Faint pencil: glyphs only slightly darker than the paper, sparse so
        // the box median stays on the paper side.
        let mut data = vec![245u8; 400 * 200];
        let mut x = 24usize;
        while x < 376 {
            for y in 83..97 {
                for xx in x..(x + 4) {
                    data[y * 400 + xx] = 215;
                }
            }
            x += 7;
        }
        let page = Gray8::new(400, 200, data).unwrap();
        let b = LineBox { x0: 20.0, y0: 78.0, x1: 380.0, y1: 102.0, score: 0.9 };
        assert!(!is_dark_line(&page, &b), "fixture must stay dark-on-light");
        let plain = crop_line(&page, &b, &CropParams::indra()).unwrap();
        let stretched = crop_line(
            &page,
            &b,
            &CropParams {
                normalize_contrast: true,
                ..CropParams::indra()
            },
        )
        .unwrap();
        // The stretch maps the 2nd..98th percentile range onto 0..=255, so the
        // faint glyphs reach full ink density while the paper stays near zero.
        let max = |c: &LineCrop| c.ink.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            max(&stretched) > max(&plain) + 0.05,
            "stretch max {} should exceed plain max {}",
            max(&stretched),
            max(&plain)
        );
        assert!(max(&stretched) > 0.95, "stretched ink should reach full density");
        assert!(mean_ink(&stretched) > mean_ink(&plain), "stretched crop is inkier overall");
    }
}
