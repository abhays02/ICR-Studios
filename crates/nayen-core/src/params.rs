//! Tunable stage parameters.
//!
//! The shipped Indra defaults are reproduced exactly in [`DetectParams::indra`],
//! so a NAYEN run with `Profile::Indra` is bit-for-bit the reference pipeline.
//! The handwriting profile changes only post-processing and crop geometry —
//! recognizer weights are never touched.
//!
//! Those handwriting values are **candidates, not proven wins**. `Profile`
//! exists so the difference is measurable: run both profiles over the same
//! ground-truth pages with `nayen-cli eval` and compare, instead of trusting a
//! constant in a source file.

use serde::{Deserialize, Serialize};

/// LineNet decode geometry, shared by training (`encode_targets`) and inference
/// (`decode`). These four are structural: changing them desynchronises decode
/// from how the model was trained, so they are not part of any profile.
pub const CORE_FRAC: f32 = 0.4;
pub const CORE_X_SHRINK: f32 = 0.15;
pub const OUT_STRIDE: usize = 2;
pub const INPUT_MULTIPLE: usize = 32;

/// `rec_preproc.LINE_HEIGHT` — the crop height every recognizer was trained on.
pub const LINE_HEIGHT: usize = 32;
/// Widths are padded to a multiple of this (the recognizer's timestep stride).
pub const WIDTH_MULTIPLE: usize = 8;
/// `crop_line`'s default cap. Beyond this a line is too wide to be one line.
pub const MAX_CROP_WIDTH: usize = 4096;

/// `pipeline.MIN_CONF`: below this mean CTC peak probability a line yields no
/// text. Its box is kept, because layout and reading order still need it.
pub const MIN_CONF: f32 = 0.5;

/// `pipeline.ADAPTIVE_*`: a median line height under `MIN_H` at the base
/// resolution triggers one re-detection pass at a larger `long_side`.
pub const ADAPTIVE_MIN_H: f32 = 9.0;
pub const ADAPTIVE_TARGET_H: f32 = 12.0;
pub const ADAPTIVE_MAX_SIDE: usize = 2048;
/// A re-detected pass only wins if it finds more lines; otherwise the first
/// result stands. Guards against a larger scale fragmenting good boxes.
pub const ADAPTIVE_MIN_BOXES: usize = 8;

/// `pipeline.ROUTE_SURE`: a line goes to a single group recognizer only when
/// the script classifier beats this. Otherwise the base recognizer also reads
/// it and the more confident result wins.
pub const ROUTE_SURE: f32 = 0.95;

/// `pipeline.ORIENT_*`. A turn is applied only above 0.95: real pages are
/// almost always upright and a wrong rotation loses the whole page.
pub const ORIENT_SIDE: usize = 768;
pub const ORIENT_MIN: f32 = 0.95;
pub const ORIENT_VERTICAL_CLASS: usize = 4;

/// `pipeline.PREVIEW_SIDE`: the skew search and `orient.onnx` read a thumbnail
/// of at most this, never the full scan.
pub const PREVIEW_SIDE: usize = 1600;
/// The skew search's own thumbnail, and the coarse grid it starts on.
pub const SKEW_SIDE: usize = 800;
/// `pipeline.DESKEW_*`: the tilt range searched, and the floor below which a
/// measured tilt is left alone (the fine step is 0.1, so anything smaller is
/// indistinguishable from noise).
pub const DESKEW_MAX: f64 = 45.0;
pub const DESKEW_MIN: f64 = 0.3;
pub const DESKEW_COARSE_STEP: f64 = 2.0;

/// Gaps between CTC character centres, measured in line heights, used to split
/// one detected line into separate fields.
pub const SPLIT_GAP: f32 = 3.0;
pub const CELL_GAP: f32 = 1.8;
pub const GUTTER_GAP: f32 = 1.6;

/// Batching limits from `rec_preproc.width_batches`.
pub const MAX_BATCH: usize = 64;
pub const MAX_WIDTH_RATIO: f32 = 1.3;
pub const MIN_BATCH_WIDTH: usize = 64;
pub const MAX_BATCH_AREA: usize = 64 * 640;

/// `det_post.decode` knobs. Every one of these is safe to vary: they only
/// change how the network's output map is turned into boxes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectParams {
    /// Sigmoid threshold on the core channel. Lower keeps fainter strokes.
    pub thresh: f32,
    /// Connected components smaller than this are dropped.
    pub min_area: usize,
    /// Grow each box by this fraction of its own height, per side, to absorb
    /// the ±1px error of the 1/2-resolution output map.
    pub grow_y: f32,
    pub grow_x: f32,
    /// Long side the page is scaled to before detection.
    pub long_side: usize,
    /// Allow one re-detection pass at a larger scale for small text.
    pub adaptive: bool,
}

impl DetectParams {
    /// The exact shipped defaults: `decode(out)` with no overrides, and
    /// `meta["det_long_side"] = 1024`.
    pub const fn indra() -> Self {
        Self {
            thresh: 0.5,
            min_area: 2,
            grow_y: 0.0,
            grow_x: 0.0,
            long_side: 1024,
            adaptive: true,
        }
    }

    /// Candidate handwriting profile.
    ///
    /// `thresh` is deliberately **left at the reference 0.5**. Lowering it is
    /// the obvious move for faint ink, and it was measured rather than assumed:
    /// running the real `det.onnx` over a three-line page, the core-channel
    /// logit distribution was median -10.1, p75 -8.5, p95 +4.7. Background
    /// probability is ~0.0000 and text ~0.99, so sweeping the threshold from
    /// 0.5 down to 0.2 changed the box count not at all (3 boxes at every
    /// value). The detector is already saturated; the threshold is not where
    /// faint handwriting is lost.
    ///
    /// Note also that `sigmoid(0) = 0.5`, so the reference threshold sits
    /// exactly on the background logit. A detector that is *not* confident
    /// would flood the mask at 0.3 and merge the whole page into one component
    /// — verified on a synthetic map with zero background logits, where 2 lines
    /// collapsed into 1. Lowering this value is only safe alongside a detector
    /// whose background logits are strongly negative.
    ///
    /// What this profile does change is safe by construction:
    /// - `min_area 1`: keeps tiny components. Cannot invent a line the network
    ///   did not mark; it only stops discarding short ones.
    /// - `grow_y` / `grow_x`: expands boxes after connected components, so it
    ///   can never merge two lines into one detection. It recovers ascenders
    ///   and descenders that sit outside the core band `CORE_FRAC = 0.4` was
    ///   fitted to on printed text.
    ///
    /// Both are still candidates. `Profile::Indra` reproduces the shipped
    /// defaults exactly so the difference is measurable on ground truth.
    pub const fn handwriting() -> Self {
        Self {
            thresh: 0.5,
            min_area: 1,
            grow_y: 0.12,
            grow_x: 0.08,
            long_side: 1024,
            adaptive: true,
        }
    }

    /// Multi-scale sweep: detect at a larger scale and merge. Costs an extra
    /// detector pass, which is why it is opt-in rather than default. The
    /// threshold is lowered slightly here because the sweep's own deduplication
    /// absorbs the extra noise a larger input produces.
    pub const fn sweep() -> Self {
        Self {
            thresh: 0.45,
            min_area: 1,
            grow_y: 0.1,
            grow_x: 0.06,
            long_side: 1024,
            adaptive: false,
        }
    }
}

/// `rec_preproc.crop_line` geometry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropParams {
    /// Vertical pad as a fraction of line height, before the window transform.
    /// `f64` because the crop geometry feeds `floor`/`ceil`: an `f32` 1.6 widens
    /// to 1.6000000089 as `f64` and shifts the crop by a pixel.
    pub pad_y: f64,
    /// Horizontal pad as a fraction of line height.
    pub pad_x: f64,
    /// Tall window: `rec.onnx` takes `[B,1,96,W]`, i.e. `window = 3`.
    /// `script.onnx` takes `[B,1,32,W]`, i.e. `window = 1`.
    pub window: usize,
    pub max_width: usize,
    /// Per-crop contrast stretch before building the ink map. Off by default so
    /// `Profile::Indra` reproduces the reference exactly.
    pub normalize_contrast: bool,
}

impl CropParams {
    pub const fn indra() -> Self {
        Self {
            pad_y: 0.2,
            pad_x: 0.3,
            window: 3,
            max_width: MAX_CROP_WIDTH,
            normalize_contrast: false,
        }
    }

    /// Wider margins for messy handwriting, plus a contrast stretch so faint
    /// pencil reaches the same dynamic range as ink the recognizer trained on.
    pub const fn handwriting() -> Self {
        Self {
            pad_y: 0.35,
            pad_x: 0.4,
            window: 3,
            max_width: MAX_CROP_WIDTH,
            normalize_contrast: true,
        }
    }

    /// `script.onnx` wants the plain 32px crop, no window.
    pub const fn script_classifier() -> Self {
        Self {
            pad_y: 0.2,
            pad_x: 0.3,
            window: 1,
            max_width: MAX_CROP_WIDTH,
            normalize_contrast: false,
        }
    }

    /// The effective vertical pad after `crop_line`'s window transform:
    /// `pad_y = (window * (1 + 2 * pad_y) - 1) / 2`.
    pub fn effective_pad_y(&self) -> f64 {
        (self.window as f64 * (1.0 + 2.0 * self.pad_y) - 1.0) / 2.0
    }

    /// The effective crop height: `LINE_HEIGHT * window`.
    pub fn effective_height(&self) -> usize {
        LINE_HEIGHT * self.window
    }
}

/// A named, reproducible configuration of the whole engine.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// The reference Indra pipeline, unmodified.
    Indra,
    /// Candidate handwriting tuning.
    Handwriting,
    /// Multi-scale detection sweep.
    Sweep,
}

impl Profile {
    pub fn detect(&self) -> DetectParams {
        match self {
            Self::Indra => DetectParams::indra(),
            Self::Handwriting => DetectParams::handwriting(),
            Self::Sweep => DetectParams::sweep(),
        }
    }

    pub fn crop(&self) -> CropParams {
        match self {
            Self::Indra | Self::Sweep => CropParams::indra(),
            Self::Handwriting => CropParams::handwriting(),
        }
    }

    /// Which stage toggles a profile changes, for telemetry and UI display.
    pub fn describes(&self) -> &'static str {
        match self {
            Self::Indra => "reference pipeline: thresh 0.5, min_area 2, no box growth, pad_y 0.2",
            Self::Handwriting => {
                "handwriting candidate: thresh 0.5, min_area 1, grow 0.12/0.08, pad_y 0.35, contrast stretch"
            }
            Self::Sweep => "multi-scale sweep: thresh 0.45, no adaptive pass, merged boxes, pad_y 0.2",
        }
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self::Indra
    }
}

/// Full engine configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConfig {
    pub profile: Profile,
    pub detect: DetectParams,
    pub crop: CropParams,
    /// Drop lines whose mean CTC confidence is below this. Matches
    /// `pipeline.MIN_CONF`; exposed so a deployment can trade recall for
    /// precision knowingly.
    pub min_conf: f32,
    /// Run `orient.onnx` and apply quarter turns. Skipped when the host did not
    /// load it.
    pub orient: bool,
    /// Search for and correct a small tilt before detection. Pure arithmetic,
    /// but ~90 rotations of a thumbnail, so it is the one stage a constrained
    /// host may want off.
    pub deskew: bool,
    /// Thumbnail the deskew search reads. Lower is faster and still resolves
    /// text rows; the reference uses 800.
    pub skew_side: usize,
    /// Run `script.onnx` and route non-Latin lines. Skipped when the host only
    /// loaded the core pair.
    pub route_scripts: bool,
    /// Run Laya's field extraction and cross-checks after recognition.
    pub laya: bool,
    /// Largest side a page is scaled to before any work. Bounds memory in the
    /// browser, where a 600dpi A3 scan would otherwise exhaust the WASM heap.
    pub max_page_side: usize,
}

impl EngineConfig {
    /// Config for the reference profile with every optional stage enabled.
    pub fn indra() -> Self {
        Self::from_profile(Profile::Indra)
    }

    pub fn from_profile(profile: Profile) -> Self {
        Self {
            profile,
            detect: profile.detect(),
            crop: profile.crop(),
            min_conf: MIN_CONF,
            orient: true,
            deskew: true,
            skew_side: SKEW_SIDE,
            route_scripts: true,
            laya: true,
            max_page_side: 2600,
        }
    }

    /// The browser preset: same maths, lower memory ceiling, and a smaller deskew
    /// thumbnail because ~90 rotations of an 800px buffer is noticeable on a
    /// phone. Optional stages stay on so a host that loaded only `det` + `rec`
    /// still works — missing models degrade, they never fabricate.
    pub fn browser(profile: Profile) -> Self {
        Self {
            max_page_side: 1800,
            skew_side: 480,
            ..Self::from_profile(profile)
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::indra()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indra_profile_reproduces_shipped_defaults() {
        let d = DetectParams::indra();
        assert_eq!((d.thresh, d.min_area, d.grow_y, d.grow_x), (0.5, 2, 0.0, 0.0));
        assert_eq!(d.long_side, 1024);
        let c = CropParams::indra();
        assert_eq!((c.pad_y, c.pad_x, c.window), (0.2, 0.3, 3));
    }

    #[test]
    fn window_transform_matches_rec_preproc() {
        // crop_line: pad_y = (window * (1 + 2 * pad_y) - 1) / 2
        let c = CropParams::indra();
        assert!((c.effective_pad_y() - (3.0 * 1.4 - 1.0) / 2.0).abs() < 1e-12);
        assert_eq!(c.effective_height(), 96); // rec.onnx input height
        assert_eq!(CropParams::script_classifier().effective_height(), 32);
    }

    #[test]
    fn profiles_differ_only_in_post_processing() {
        // Structural geometry must stay identical across profiles.
        assert_eq!(CORE_FRAC, 0.4);
        assert_eq!(OUT_STRIDE, 2);
        assert_eq!(LINE_HEIGHT, 32);
        assert_ne!(Profile::Indra.detect(), Profile::Handwriting.detect());
        assert_ne!(Profile::Indra.crop(), Profile::Handwriting.crop());
        // The handwriting profile must not silently move the threshold; that was
        // measured to be a no-op at best and a page-merging hazard at worst.
        assert_eq!(Profile::Indra.detect().thresh, Profile::Handwriting.detect().thresh);
    }

    #[test]
    fn config_round_trips_through_serde() {
        let c = EngineConfig::from_profile(Profile::Handwriting);
        let json = serde_json::to_string(&c).unwrap();
        let back: EngineConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        assert!(json.contains("handwriting"));
    }

    #[test]
    fn describes_quotes_the_numbers_it_actually_uses() {
        // These strings are copied into web/src/demo.ts and shown to users as the
        // explanation of what a profile does. A stale number there is a lie in the
        // UI, so the numbers are checked against the params rather than trusted.
        for profile in [Profile::Indra, Profile::Handwriting, Profile::Sweep] {
            let text = profile.describes();
            let detect = profile.detect();
            let crop = profile.crop();
            assert!(
                text.contains(&format!("thresh {}", detect.thresh)),
                "{text:?} does not quote thresh {}",
                detect.thresh
            );
            assert!(
                text.contains(&format!("pad_y {}", crop.pad_y)),
                "{text:?} does not quote pad_y {}",
                crop.pad_y
            );
            if detect.min_area == 2 {
                assert!(text.contains("min_area 2"), "{text:?}");
            }
        }
    }
}
