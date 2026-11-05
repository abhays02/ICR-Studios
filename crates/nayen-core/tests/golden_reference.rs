//! Golden cross-check against the reference implementation.
//!
//! Every expected value in this file was produced by the real `indra-ocr`
//! Python package (`det_post.page_to_input`, `det_post.decode`,
//! `rec_preproc.crop_line`, `rec_preproc.is_dark_line`, `rec_preproc.pad_batch`,
//! `rec_preproc.width_batches`) on the deterministic pages built below.
//!
//! This is the test that makes the port trustworthy. Shapes, box counts and box
//! coordinates must match exactly — they are pure integer arithmetic. Ink means
//! are compared with a tolerance because cv2 and this crate use slightly
//! different bilinear fixed-point rounding (measured: mean absolute pixel
//! difference ~1/255 against `cv2.warpAffine`).
//!
//! To regenerate: `python3 tests/golden/generate.py` (see that file's docstring).

use nayen_core::crop::{crop_line, is_dark_line, pad_batch, width_batches};
use nayen_core::det::{decode, LineBox};
use nayen_core::image::{page_to_input, Gray8};
use nayen_core::params::{CropParams, DetectParams, WIDTH_MULTIPLE};

const BG: u8 = 250;
const INK: u8 = 25;
const LINE_ROWS: [(usize, usize); 3] = [(40, 62), (100, 122), (160, 182)];

/// The reference page: three rows of sparse dark glyph strokes on light paper.
/// Sparse on purpose, so a detected box contains paper as well as ink — the
/// condition `is_dark_line` and `crop_line` are designed around.
fn page() -> Gray8 {
    let (h, w) = (220usize, 400usize);
    let mut d = vec![BG; w * h];
    for &(y0, y1) in &LINE_ROWS {
        let mut x = 24usize;
        while x < w - 24 {
            for yy in (y0 + 3)..(y1 - 3) {
                for xx in x..(x + 4).min(w) {
                    d[yy * w + xx] = INK;
                }
            }
            x += 7;
        }
    }
    Gray8::new(w, h, d).unwrap()
}

/// A dark banner with thin light glyphs: the light-on-dark case the recognizers
/// cannot read without inversion.
fn banner() -> Gray8 {
    let (h, w) = (220usize, 400usize);
    let mut d = vec![BG; w * h];
    for y in 30..90 {
        for x in 0..w {
            d[y * w + x] = 28;
        }
    }
    let mut x = 40usize;
    while x < 360 {
        for y in 45..75 {
            for xx in x..(x + 4) {
                d[y * w + xx] = 235;
            }
        }
        x += 9;
    }
    Gray8::new(w, h, d).unwrap()
}

fn line_box() -> LineBox {
    LineBox {
        x0: 20.0,
        y0: 38.0,
        x1: 380.0,
        y1: 64.0,
        score: 1.0,
    }
}

fn banner_box() -> LineBox {
    LineBox {
        x0: 20.0,
        y0: 30.0,
        x1: 380.0,
        y1: 90.0,
        score: 1.0,
    }
}

fn crop(w: usize, pad_y: f64, pad_x: f64) -> CropParams {
    CropParams {
        pad_y,
        pad_x,
        window: w,
        ..CropParams::indra()
    }
}

/// Relative-tolerance comparison for values that pass through resampling.
fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
    let denom = expected.abs().max(1e-9);
    let rel = (actual - expected).abs() / denom;
    assert!(
        rel <= tol,
        "{what}: got {actual}, reference {expected} (relative error {rel:.4} > {tol})"
    );
}

#[test]
fn page_to_input_matches_reference() {
    let (ink, scale) = page_to_input(&page(), 256).unwrap();
    assert_eq!((ink.height, ink.width), (160, 256), "padded input shape");
    assert_close(scale, 0.64, 1e-9, "scale");
    assert_close(ink.data.iter().map(|&v| v as f64).sum::<f64>(), 4251.9052734375, 0.02, "ink sum");
    assert_close(
        ink.data.iter().map(|&v| v as f64).sum::<f64>() / ink.data.len() as f64,
        0.1038062795996666,
        0.02,
        "ink mean",
    );
}

#[test]
fn is_dark_line_matches_reference() {
    assert_eq!(is_dark_line(&page(), &line_box()), false, "reference: false");
    assert_eq!(is_dark_line(&banner(), &banner_box()), true, "reference: true");
}

#[test]
fn crop_line_geometry_matches_reference_exactly() {
    let p = page();
    // window=3 -> rec.onnx's 96px input; width is pure arithmetic, so it must
    // match to the pixel.
    let c = crop_line(&p, &line_box(), &crop(3, 0.2, 0.3)).unwrap();
    assert_eq!((c.height, c.width), (96, 328), "crop window=3");
    assert_close(c.ink.iter().map(|&v| v as f64).sum::<f64>() / c.ink.len() as f64, 0.10160409659147263, 0.03, "w3 mean");
    assert_close(
        c.ink.iter().cloned().fold(f32::MIN, f32::max) as f64,
        0.9019607901573181,
        1e-6,
        "w3 max is 1 - 25/255",
    );
    assert_eq!(c.ink.iter().cloned().fold(f32::MAX, f32::min), 0.0, "w3 min");

    // window=1 -> script.onnx's 32px input.
    let c1 = crop_line(&p, &line_box(), &crop(1, 0.2, 0.3)).unwrap();
    assert_eq!((c1.height, c1.width), (32, 317), "crop window=1");
    assert_close(c1.ink.iter().map(|&v| v as f64).sum::<f64>() / c1.ink.len() as f64, 0.22119133174419403, 0.03, "w1 mean");

    // The handwriting crop: a taller window at the same output height shrinks
    // the glyph scale, so the crop is *narrower*. Measured, not assumed.
    let ch = crop_line(&p, &line_box(), &crop(3, 0.35, 0.4)).unwrap();
    assert_eq!((ch.height, ch.width), (96, 274), "crop handwriting");
    assert!(
        ch.width < c.width,
        "pad_y 0.35 shrinks glyph scale: {} vs {} wide",
        ch.width,
        c.width
    );
    assert_close(ch.ink.iter().map(|&v| v as f64).sum::<f64>() / ch.ink.len() as f64, 0.12630078196525574, 0.03, "hw mean");
}

#[test]
fn dark_banner_crop_is_inverted() {
    let c = crop_line(&banner(), &banner_box(), &crop(3, 0.2, 0.3)).unwrap();
    assert!(c.dark, "the banner must be flagged light-on-dark");
    assert_eq!((c.height, c.width), (96, 151), "banner crop shape");
    assert_close(
        c.ink.iter().map(|&v| v as f64).sum::<f64>() / c.ink.len() as f64,
        0.5514844059944153,
        0.03,
        "banner ink mean: inverted so glyphs stay ink=1",
    );
}

#[test]
fn pad_batch_matches_reference() {
    let p = page();
    let crops: Vec<_> = LINE_ROWS
        .iter()
        .map(|&(y0, y1)| {
            crop_line(
                &p,
                &LineBox {
                    x0: 20.0,
                    y0: (y0 - 2) as f32,
                    x1: 380.0,
                    y1: (y1 + 2) as f32,
                    score: 1.0,
                },
                &crop(3, 0.2, 0.3),
            )
            .unwrap()
        })
        .collect();
    let refs: Vec<&_> = crops.iter().collect();
    let (x, widths) = pad_batch(&refs).unwrap();
    assert_eq!(x.tensor.shape, vec![3, 1, 96, 328], "reference shape [3,1,96,328]");
    assert_eq!(x.tensor.shape[3] % WIDTH_MULTIPLE as i64, 0);
    assert_eq!(widths.tensor.data.as_i64().unwrap(), &[328i64, 328, 328]);
    let data = x.tensor.data.as_f32().unwrap();
    assert_close(
        data.iter().map(|&v| v as f64).sum::<f64>() / data.len() as f64,
        0.10606450587511063,
        0.03,
        "padded batch mean",
    );
}

#[test]
fn pad_batch_padding_is_paper() {
    let p = page();
    let wide = crop_line(&p, &line_box(), &crop(3, 0.2, 0.3)).unwrap();
    let narrow = crop_line(
        &p,
        &LineBox {
            x0: 20.0,
            y0: 38.0,
            x1: 120.0,
            y1: 64.0,
            score: 1.0,
        },
        &crop(3, 0.2, 0.3),
    )
    .unwrap();
    assert!(narrow.width < wide.width);
    let (x, widths) = pad_batch(&[&wide, &narrow]).unwrap();
    let wm = x.tensor.shape[3] as usize;
    let w1 = widths.tensor.data.as_i64().unwrap()[1] as usize;
    let data = x.tensor.data.as_f32().unwrap();
    assert!(
        data[1 * 96 * wm + w1..1 * 96 * wm + wm]
            .iter()
            .all(|&v| v == 0.0),
        "padding columns must be paper, not stale memory"
    );
}

#[test]
fn width_batches_matches_reference_exactly() {
    // Reference: [[2, 0, 3, 1], [4, 5]] — narrowest first, wide ones split off.
    assert_eq!(width_batches(&[100, 102, 98, 101, 900, 910]), vec![vec![2, 0, 3, 1], vec![4, 5]]);
    // Reference: 133 equal widths -> 3 batches (64 + 64 + 5).
    assert_eq!(width_batches(&[64usize; 133]).len(), 3);
    // Reference: 64 lines of 4096px -> [10,10,10,10,10,10,4], the area cap biting.
    let sizes: Vec<usize> = width_batches(&[4096usize; 64]).iter().map(|b| b.len()).collect();
    assert_eq!(sizes, vec![10, 10, 10, 10, 10, 10, 4]);
}

/// The synthetic `[3,h,w]` detector output the reference `decode` was run on.
fn synthetic_det(faint: bool) -> (Vec<f32>, usize, usize) {
    let (h, w) = (24usize, 48usize);
    let plane = h * w;
    let mut out = vec![0f32; 3 * plane];
    let logit = if faint { -0.4 } else { 5.0 };
    for &(y0, y1) in &[(6usize, 10usize), (16, 20)] {
        for y in y0..y1 {
            for x in 5..40 {
                let i = y * w + x;
                out[i] = logit;
                out[plane + i] = ((y - y0) as f32 + 0.5) * 2.0;
                out[2 * plane + i] = ((y1 - 1 - y) as f32 + 0.5) * 2.0;
            }
        }
    }
    (out, h, w)
}

#[test]
fn decode_matches_reference_boxes_exactly() {
    let (out, h, w) = synthetic_det(false);
    let boxes = decode(&out, h, w, &DetectParams::indra());
    assert_eq!(boxes.len(), 2, "reference: 2 boxes");
    // Reference: [[8.8, 12.0, 81.2, 20.0, 0.9933], [8.8, 32.0, 81.2, 40.0, 0.9933]]
    let want = [
        [8.8f32, 12.0, 81.2, 20.0, 0.9933],
        [8.8, 32.0, 81.2, 40.0, 0.9933],
    ];
    for (b, w4) in boxes.iter().zip(want.iter()) {
        let got = [b.x0, b.y0, b.x1, b.y1, b.score];
        for (i, (g, e)) in got.iter().zip(w4.iter()).enumerate() {
            assert!(
                (g - e).abs() < 5e-4,
                "box component {i}: got {g}, reference {e}"
            );
        }
    }
}

#[test]
fn decode_with_growth_matches_reference() {
    let (out, h, w) = synthetic_det(false);
    let p = DetectParams {
        min_area: 1,
        grow_y: 0.12,
        grow_x: 0.08,
        ..DetectParams::indra()
    };
    let boxes = decode(&out, h, w, &p);
    assert_eq!(boxes.len(), 2, "growth must not merge separate lines");
    // Reference: [[8.16, 11.04, 81.84, 20.96, 0.9933], [8.16, 31.04, 81.84, 40.96, 0.9933]]
    let want = [
        [8.16f32, 11.04, 81.84, 20.96],
        [8.16, 31.04, 81.84, 40.96],
    ];
    for (b, w4) in boxes.iter().zip(want.iter()) {
        let got = [b.x0, b.y0, b.x1, b.y1];
        for (i, (g, e)) in got.iter().zip(w4.iter()).enumerate() {
            assert!((g - e).abs() < 5e-4, "grown box component {i}: got {g}, reference {e}");
        }
        assert!((b.score - 0.9933).abs() < 5e-4);
    }
}

#[test]
fn decode_threshold_hazard_matches_reference() {
    // Reference on a faint map (core logit -0.4, sigmoid ~0.401):
    //   decode(det, thresh=0.5) -> 0 boxes
    //   decode(det, thresh=0.3, min_area=1) -> 1 box
    // The single box is the whole page merged, which is the hazard documented on
    // DetectParams::handwriting.
    let (out, h, w) = synthetic_det(true);
    assert_eq!(decode(&out, h, w, &DetectParams::indra()).len(), 0);
    let lo = DetectParams {
        thresh: 0.3,
        min_area: 1,
        ..DetectParams::indra()
    };
    assert_eq!(decode(&out, h, w, &lo).len(), 1);
}
