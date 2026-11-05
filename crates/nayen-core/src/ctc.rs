//! CTC greedy decoding for the recognizer's raw per-timestep output.
//!
//! `rec.onnx` does *not* collapse its output. It returns `ids[B,T]` (the
//! argmax class at every timestep, blanks and repeats included), `conf[B,T]`
//! (that class's probability) and `lengths[B]` (how many timesteps are real).
//! The collapse — drop blank, merge consecutive repeats — is host code, in the
//! reference implementation and here.
//!
//! Getting this wrong is invisible: a missed dedupe doubles letters, a missed
//! blank filter injects noise characters. Both still look like plausible text.

use serde::{Deserialize, Serialize};

/// One decoded line, before any confidence gate is applied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CtcReading {
    /// Class ids after collapse, in reading order.
    pub ids: Vec<i64>,
    /// Timestep index each id came from, for character-position mapping.
    pub steps: Vec<usize>,
    /// Mean peak probability over the kept timesteps.
    pub conf: f32,
}

impl CtcReading {
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Collapse one line's raw output.
///
/// `ids` and `conf` are the row slices already truncated to `lengths[b]`.
/// The keep mask is `(id != blank) && (first || id != previous)`, which is
/// standard CTC greedy decoding and matches the reference exactly.
pub fn collapse(ids: &[i64], conf: &[f32], blank: i64) -> CtcReading {
    let n = ids.len().min(conf.len());
    let mut out_ids = Vec::new();
    let mut out_steps = Vec::new();
    let mut sum = 0f64;

    for t in 0..n {
        let id = ids[t];
        if id == blank {
            continue;
        }
        // Merge a run of the same class: "ll" in handwriting is one timestep
        // per glyph only if the network actually emitted a blank between them.
        if t > 0 && ids[t - 1] == id {
            continue;
        }
        out_ids.push(id);
        out_steps.push(t);
        sum += conf[t] as f64;
    }

    let count = out_ids.len();
    CtcReading {
        ids: out_ids,
        steps: out_steps,
        conf: if count == 0 {
            0.0
        } else {
            (sum / count as f64) as f32
        },
    }
}

/// Map kept timesteps to page-x coordinates, for drawing a per-character
/// overlay that lines up with the original scan.
///
/// `crop_width` is the crop's width *before* batch padding, because the
/// recognizer's timestep grid spans the padded tensor while the ink only
/// occupies the first `crop_width` columns.
pub fn char_positions(
    steps: &[usize],
    timesteps: usize,
    padded_width: usize,
    crop_width: usize,
    box_x0: f32,
    box_x1: f32,
    line_height: f64,
    pad_x: f64,
) -> Vec<f32> {
    if steps.is_empty() || timesteps == 0 || crop_width == 0 {
        return Vec::new();
    }
    // Input columns per output timestep.
    let stride = padded_width as f32 / timesteps as f32;
    // The crop's own left edge and width in page pixels, matching crop_line.
    let h = line_height.max(1.0);
    let ix0 = (box_x0 as f64 - pad_x * h).floor();
    let cw = (box_x1 as f64 + pad_x * h).ceil() - ix0;
    let f = (cw / crop_width as f64) as f32;
    steps
        .iter()
        .map(|&t| (ix0 + (t as f64 + 0.5) * stride as f64 * f as f64) as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_are_dropped() {
        let r = collapse(&[0, 1, 0, 2, 0], &[0.9, 0.8, 0.9, 0.7, 0.9], 0);
        assert_eq!(r.ids, vec![1, 2]);
        assert_eq!(r.steps, vec![1, 3]);
        assert!((r.conf - 0.75).abs() < 1e-6, "mean of 0.8 and 0.7");
    }

    #[test]
    fn consecutive_repeats_merge() {
        // "aa" written as one long stroke: timesteps 1,2,3 all class 1.
        let r = collapse(&[0, 1, 1, 1, 0], &[0.9, 0.8, 0.85, 0.8, 0.9], 0);
        assert_eq!(r.ids, vec![1], "a run collapses to one character");
        assert!((r.conf - 0.8).abs() < 1e-6, "only the first of the run is kept");
    }

    #[test]
    fn repeat_separated_by_blank_survives() {
        // "aa" as two distinct glyphs: the blank between them is meaningful.
        let r = collapse(&[1, 0, 1], &[0.8, 0.9, 0.7], 0);
        assert_eq!(r.ids, vec![1, 1], "blank-separated repeats are two characters");
        assert!((r.conf - 0.75).abs() < 1e-6);
    }

    #[test]
    fn all_blank_yields_empty_reading_with_zero_confidence() {
        let r = collapse(&[0, 0, 0], &[0.99, 0.99, 0.99], 0);
        assert!(r.is_empty());
        assert_eq!(r.conf, 0.0);
    }

    #[test]
    fn empty_input_is_safe() {
        let r = collapse(&[], &[], 0);
        assert!(r.is_empty());
        assert_eq!(r.conf, 0.0);
        assert_eq!(char_positions(&[], 0, 0, 0, 0.0, 0.0, 0.0, 0.0).len(), 0);
    }

    #[test]
    fn mismatched_slices_use_the_shorter() {
        let r = collapse(&[1, 2, 3], &[0.5, 0.5], 0);
        assert_eq!(r.ids, vec![1, 2]);
    }

    #[test]
    fn char_positions_are_monotonic_and_inside_the_box() {
        let steps = vec![0, 5, 10, 15];
        let xs = char_positions(&steps, 20, 200, 200, 100.0, 300.0, 20.0, 0.3);
        assert_eq!(xs.len(), 4);
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "left to right: {xs:?}");
        // pad_x expands the crop, so positions may sit slightly outside the box.
        assert!(xs[0] >= 90.0 && xs[3] <= 310.0, "{xs:?}");
    }

    #[test]
    fn timestep_grid_is_rescaled_onto_the_crop_width() {
        // The recognizer's timestep grid spans the *padded* tensor, but the ink
        // only occupies the first `crop_width` columns. The reference rescales by
        // f = crop_box_width / crop_width, so a narrower crop maps the same
        // timesteps across the same physical box — the spread grows, because each
        // timestep now covers more page pixels.
        let steps = vec![0, 10];
        let wide = char_positions(&steps, 20, 200, 200, 0.0, 100.0, 20.0, 0.3);
        let narrow = char_positions(&steps, 20, 200, 100, 0.0, 100.0, 20.0, 0.3);
        assert!(
            narrow[1] > wide[1],
            "a half-width crop doubles the per-timestep spread: {} vs {}",
            narrow[1],
            wide[1]
        );
        // Both start at the crop's padded left edge; only the per-timestep step
        // differs, so the gap between consecutive characters scales with f.
        let gap = |v: &[f32]| v[1] - v[0];
        assert!(
            (gap(&narrow) - 2.0 * gap(&wide)).abs() < 1e-3,
            "half-width crop doubles the step: {} vs {}",
            gap(&narrow),
            gap(&wide)
        );
    }
}
