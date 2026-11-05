//! Page geometry: rotation, the deskew search, and `orient.onnx`'s input.
//!
//! All of it is pure arithmetic on the grayscale page, so it runs identically
//! native and in WASM.

use crate::image::Gray8;
use crate::params::{ORIENT_SIDE, PREVIEW_SIDE};

/// Shrink the page once to at most `side` pixels a side. The skew search and
/// the orientation model read this instead of the full scan: a 600dpi A3 page
/// is 20 megapixels and neither needs that resolution.
pub fn preview(gray: &Gray8, side: usize) -> Gray8 {
    let longest = gray.width.max(gray.height);
    if longest <= side || longest == 0 {
        return gray.clone();
    }
    let s = side as f64 / longest as f64;
    let nw = ((gray.width as f64 * s).round() as usize).max(1);
    let nh = ((gray.height as f64 * s).round() as usize).max(1);
    gray.resize(nw, nh).unwrap_or_else(|_| gray.clone())
}

/// An ink thumbnail: 255 = ink, 0 = paper. Eight-bit images rotate faster than
/// float ones, which is why the search uses this instead of an [`InkMap`].
pub fn ink_thumb(gray: &Gray8, side: usize) -> Gray8 {
    let small = preview(gray, side);
    let data: Vec<u8> = small.data.iter().map(|&v| 255 - v).collect();
    Gray8 {
        width: small.width,
        height: small.height,
        data,
    }
}

/// `orient_input`: a `[side, side]` float ink map, long side scaled to `side`
/// and top-left aligned. Unlike `page_to_input` it is *not* padded to a multiple
/// of 32 — the model wants an exact square.
pub fn orient_input(gray: &Gray8, side: usize) -> Vec<f32> {
    let small = preview(gray, side);
    let mut out = vec![0f32; side * side];
    for y in 0..small.height {
        let dst = y * side;
        let src = y * small.width;
        for x in 0..small.width {
            out[dst + x] = 1.0 - small.data[src + x] as f32 / 255.0;
        }
    }
    out
}

/// Rotate counter-clockwise by `degrees` about the centre, same output size,
/// bilinear, with edge pixels replicated — matching `cv2.warpAffine` with
/// `BORDER_REPLICATE`, which keeps the page margins from turning into black
/// wedges that the detector would then read as ink.
pub fn rotate(gray: &Gray8, degrees: f64) -> Gray8 {
    if degrees.abs() < 1e-9 {
        return gray.clone();
    }
    let th = degrees.to_radians();
    let (a, b) = (th.cos(), th.sin());
    let cx = gray.width as f64 / 2.0;
    let cy = gray.height as f64 / 2.0;
    let (w, h) = (gray.width, gray.height);
    let mut data = vec![0u8; w * h];
    let max_x = (w - 1) as f64;
    let max_y = (h - 1) as f64;

    for y in 0..h {
        let dy = y as f64 + 0.5 - cy;
        for x in 0..w {
            let dx = x as f64 + 0.5 - cx;
            // Inverse of getRotationMatrix2D's linear part.
            let sx = cx + a * dx - b * dy;
            let sy = cy + b * dx + a * dy;
            data[y * w + x] = sample_bilinear_replicate(gray, sx - 0.5, sy - 0.5, max_x, max_y);
        }
    }
    Gray8 {
        width: w,
        height: h,
        data,
    }
}

fn sample_bilinear_replicate(g: &Gray8, fx: f64, fy: f64, max_x: f64, max_y: f64) -> u8 {
    let fx = fx.clamp(0.0, max_x);
    let fy = fy.clamp(0.0, max_y);
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(g.width - 1);
    let y1 = (y0 + 1).min(g.height - 1);
    let wx = fx - x0 as f64;
    let wy = fy - y0 as f64;
    let p00 = g.data[y0 * g.width + x0] as f64;
    let p01 = g.data[y0 * g.width + x1] as f64;
    let p10 = g.data[y1 * g.width + x0] as f64;
    let p11 = g.data[y1 * g.width + x1] as f64;
    let top = p00 + (p01 - p00) * wx;
    let bot = p10 + (p11 - p10) * wx;
    (top + (bot - top) * wy).round().clamp(0.0, 255.0) as u8
}

/// Exact quarter turns. `k` is `orient.onnx`'s class: counter-clockwise turns
/// needed to bring the page upright. Dimensions swap on the odd turns, which
/// `rotate` deliberately does not do.
pub fn rotate_quarter(gray: &Gray8, k: usize) -> Gray8 {
    match k % 4 {
        0 => gray.clone(),
        2 => {
            let mut data = vec![0u8; gray.width * gray.height];
            let n = gray.data.len();
            for i in 0..n {
                data[i] = gray.data[n - 1 - i];
            }
            Gray8 {
                width: gray.width,
                height: gray.height,
                data,
            }
        }
        _ => {
            // 90 degrees counter-clockwise. The output is h wide and w tall, and
            // dst(x, y) reads src(col = w-1-y, row = x).
            let (w, h) = (gray.width, gray.height);
            let mut data = vec![0u8; w * h];
            for y in 0..w {
                for x in 0..h {
                    data[y * h + x] = gray.data[x * w + (w - 1 - y)];
                }
            }
            Gray8 {
                width: h,
                height: w,
                data,
            }
        }
    }
}

/// `_row_sharpness`: how sharply the horizontal ink profile changes row to row
/// once the page is turned by `a` degrees. Text rows that are level produce
/// high-contrast row sums; a tilted page smears them together. Maximising this
/// is the whole deskew heuristic.
pub fn row_sharpness(ink: &Gray8, degrees: f64) -> f64 {
    let r = rotate(ink, degrees);
    let mut prev: Option<i64> = None;
    let mut acc = 0f64;
    for y in 0..r.height {
        let row = &r.data[y * r.width..(y + 1) * r.width];
        let sum: i64 = row.iter().map(|&v| v as i64).sum();
        if let Some(p) = prev {
            let d = (sum - p) as f64;
            acc += d * d;
        }
        prev = Some(sum);
    }
    acc
}

/// `estimate_skew`: degrees counter-clockwise that straighten the text rows.
///
/// Coarse-to-fine, exactly like the reference: 2-degree steps over
/// ±`max_degrees` on a half-size thumbnail, then 0.5-degree refinement around
/// the two best angles and zero, then 0.1-degree around the winner. The coarse
/// pass is what makes this affordable — a naive 0.1-degree search over the same
/// range is 900 rotations instead of ~90.
pub fn estimate_skew(gray: &Gray8, side: usize, max_degrees: f64, step_coarse: f64) -> f64 {
    let fine = ink_thumb(gray, side);
    // A blank page has no rows to straighten, and scoring one would return noise.
    let mean = fine.data.iter().map(|&v| v as u64).sum::<u64>() as f64 / fine.data.len().max(1) as f64;
    if mean < 0.003 * 255.0 {
        return 0.0;
    }
    let coarse_img = ink_thumb(gray, side / 2);

    let mut angles: Vec<f64> = Vec::new();
    let mut a = -max_degrees;
    while a <= max_degrees + 1e-6 {
        angles.push(a);
        a += step_coarse;
    }
    let mut scored: Vec<(f64, f64)> = angles
        .iter()
        .map(|&ang| (ang, row_sharpness(&coarse_img, ang)))
        .collect();
    scored.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));

    // Refine around zero as well: an already-level page must stay level, and the
    // coarse grid's best angle can be an artefact of a single strong rule.
    let mut starts = vec![0.0f64];
    for (ang, _) in scored.iter().take(2) {
        if !starts.contains(ang) {
            starts.push(*ang);
        }
    }

    let mut best = (0.0f64, f64::NEG_INFINITY);
    let mut seen: Vec<(f64, f64)> = Vec::new();
    for &s in &starts {
        let mut a = s - step_coarse;
        while a <= s + step_coarse + 1e-6 {
            let key = (a * 10.0).round() / 10.0;
            if !seen.iter().any(|(k, _)| (k - key).abs() < 1e-9) {
                let sc = row_sharpness(&fine, key);
                seen.push((key, sc));
                if sc > best.1 {
                    best = (key, sc);
                }
            }
            a += 0.5;
        }
    }

    // Final 0.1-degree pass around the winner.
    let mut a = best.0 - 0.5;
    let mut out = best;
    while a <= best.0 + 0.5 + 1e-6 {
        let key = (a * 10.0).round() / 10.0;
        let sc = row_sharpness(&fine, key);
        if sc > out.1 {
            out = (key, sc);
        }
        a += 0.1;
    }
    out.0
}

/// Map a box found on a page that `rotate(.., degrees)` produced back to the
/// page before it: the bounding box of the four mapped corners, clipped.
pub fn unrotate_box(
    box_: [f32; 4],
    degrees: f64,
    (w, h): (usize, usize),
) -> [f32; 4] {
    if degrees.abs() < 1e-9 {
        return box_;
    }
    let th = (-degrees).to_radians();
    let (a, b) = (th.cos(), th.sin());
    let cx = w as f64 / 2.0;
    let cy = h as f64 / 2.0;
    let [x0, y0, x1, y1] = box_.map(|v| v as f64);
    let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (px, py) in corners {
        let dx = px - cx;
        let dy = py - cy;
        let sx = cx + a * dx - b * dy;
        let sy = cy + b * dx + a * dy;
        min_x = min_x.min(sx);
        min_y = min_y.min(sy);
        max_x = max_x.max(sx);
        max_y = max_y.max(sy);
    }
    [
        ((min_x.clamp(0.0, w as f64) * 10.0).round() / 10.0) as f32,
        ((min_y.clamp(0.0, h as f64) * 10.0).round() / 10.0) as f32,
        ((max_x.clamp(0.0, w as f64) * 10.0).round() / 10.0) as f32,
        ((max_y.clamp(0.0, h as f64) * 10.0).round() / 10.0) as f32,
    ]
}

/// The default thumbnail the skew search and orientation model read.
pub fn default_preview_side() -> usize {
    PREVIEW_SIDE
}

/// The square side `orient.onnx` expects.
pub fn default_orient_side() -> usize {
    ORIENT_SIDE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of horizontal black bars: level text rows, a known-good skew of 0.
    fn barred_page(w: usize, h: usize, rows: &[(usize, usize)]) -> Gray8 {
        let mut data = vec![255u8; w * h];
        for &(y0, y1) in rows {
            for y in y0..y1.min(h) {
                for x in 10..w.saturating_sub(10) {
                    data[y * w + x] = 0;
                }
            }
        }
        Gray8::new(w, h, data).unwrap()
    }

    #[test]
    fn preview_never_upscales() {
        let g = barred_page(200, 100, &[(20, 30)]);
        let p = preview(&g, 400);
        assert_eq!((p.width, p.height), (200, 100));
        let p = preview(&g, 100);
        assert_eq!(p.width.max(p.height), 100);
    }

    #[test]
    fn ink_thumb_inverts() {
        let g = Gray8::filled(8, 8, 255).unwrap();
        let t = ink_thumb(&g, 8);
        assert!(t.data.iter().all(|&v| v == 0), "white page has no ink");
        let g = Gray8::filled(8, 8, 0).unwrap();
        let t = ink_thumb(&g, 8);
        assert!(t.data.iter().all(|&v| v == 255));
    }

    #[test]
    fn orient_input_is_exactly_square_and_ink_scaled() {
        let g = barred_page(300, 200, &[(50, 60)]);
        let o = orient_input(&g, 64);
        assert_eq!(o.len(), 64 * 64);
        assert!(o.iter().all(|v| *v >= 0.0 && *v <= 1.0));
        // Top-left aligned page means the bottom-right padding is paper.
        assert_eq!(o[64 * 64 - 1], 0.0);
    }

    #[test]
    fn quarter_turn_swaps_dimensions_and_is_cyclic() {
        let g = barred_page(120, 80, &[(20, 30)]);
        let r1 = rotate_quarter(&g, 1);
        assert_eq!((r1.width, r1.height), (80, 120));
        assert_eq!(rotate_quarter(&g, 0), g);
        assert_eq!(rotate_quarter(&g, 4), g);
        let r2 = rotate_quarter(&g, 2);
        assert_eq!((r2.width, r2.height), (120, 80));
        // 180 degrees reverses the buffer.
        assert_eq!(r2.data[0], g.data[g.data.len() - 1]);
    }

    #[test]
    fn small_rotation_is_near_identity() {
        let g = barred_page(80, 60, &[(20, 30)]);
        let r = rotate(&g, 0.0);
        assert_eq!(r, g, "zero degrees must not resample");
        let r = rotate(&g, 0.5);
        // A half-degree turn of a barred page changes very few pixels.
        let diff = r
            .data
            .iter()
            .zip(g.data.iter())
            .filter(|(a, b)| (**a as i32 - **b as i32).abs() > 24)
            .count();
        assert!(diff < g.data.len() / 10, "only edges should move, {diff} changed");
    }

    #[test]
    fn row_sharpness_peaks_at_the_correcting_angle() {
        // `rotate` matches cv2.getRotationMatrix2D's convention (verified to
        // ~1/255 mean absolute pixel difference), so rotating by +6 and then by
        // -6 restores the page. Sharpness must peak at the inverse angle, not at
        // the applied one — that is the whole basis of the deskew search.
        let level = barred_page(240, 160, &[(40, 46), (70, 76), (100, 106)]);
        let tilt = 6.0;
        let tilted = rotate(&level, tilt);
        let ink = ink_thumb(&tilted, 240);
        let at_zero = row_sharpness(&ink, 0.0);
        let at_correction = row_sharpness(&ink, -tilt);
        let at_wrong_sign = row_sharpness(&ink, tilt);
        assert!(
            at_correction > at_zero,
            "undoing the tilt must sharpen the rows: {at_correction} vs {at_zero}"
        );
        assert!(
            at_correction > at_wrong_sign,
            "the inverse angle beats the applied one: {at_correction} vs {at_wrong_sign}"
        );
        // A level page is already at its own maximum.
        let level_ink = ink_thumb(&level, 240);
        assert!(row_sharpness(&level_ink, 0.0) > row_sharpness(&level_ink, tilt));
    }

    #[test]
    fn blank_page_has_zero_skew() {
        let g = Gray8::filled(200, 200, 255).unwrap();
        assert_eq!(estimate_skew(&g, 128, 45.0, 2.0), 0.0);
    }

    #[test]
    fn estimate_skew_returns_the_angle_that_straightens() {
        // `estimate_skew` returns the angle to APPLY, so for a page produced by
        // rotate(level, +7) it must come back near -7, and applying it must
        // restore row sharpness. Asserting the round trip rather than a sign
        // convention keeps this correct even if `rotate`'s convention changes.
        let level = barred_page(320, 220, &[(50, 56), (90, 96), (130, 136), (170, 176)]);
        let tilt = 7.0;
        let tilted = rotate(&level, tilt);
        let est = estimate_skew(&tilted, 160, 20.0, 2.0);
        let want = -tilt;
        assert!(
            (est - want).abs() <= 1.5,
            "expected ~{want}, estimated {est}"
        );
        let ink_before = ink_thumb(&tilted, 320);
        let ink_after = ink_thumb(&rotate(&tilted, est), 320);
        assert!(
            row_sharpness(&ink_after, 0.0) > row_sharpness(&ink_before, 0.0),
            "applying the estimate must straighten the rows"
        );
        // A level page must be left alone: DESKEW_MIN exists so noise does not
        // trigger a resample of an already-correct page.
        let flat = estimate_skew(&level, 160, 20.0, 2.0);
        assert!(flat.abs() < 1.5, "a level page estimated {flat}");
    }

    #[test]
    fn unrotate_box_round_trips() {
        let b = [100.0, 50.0, 200.0, 90.0];
        let back = unrotate_box(b, 0.0, (400, 300));
        assert_eq!(back, b);
        // A 90-degree turn maps a wide box to a tall one.
        let turned = unrotate_box([10.0, 10.0, 110.0, 30.0], 90.0, (200, 200));
        assert!(turned[3] - turned[1] > turned[2] - turned[0], "{turned:?}");
    }
}
