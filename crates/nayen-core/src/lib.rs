//! # NAYEN v1
//!
//! A native intelligent-character-recognition engine for handwritten banking
//! forms. NAYEN v1 integrates two layers that are usually separate:
//!
//! - **Indra** — perception. Page orientation, line detection, line cropping and
//!   CTC character recognition, using the `datalab-to/indra-ocr` model family.
//! - **Laya** — decision. Script routing, confidence gating, form-field
//!   extraction, rule validation and cross-field consistency, plus the review
//!   flags that drive a human queue.
//!
//! ## Why the core links no inference runtime
//!
//! Every tensor operation goes through [`tensor::Executor`], and the pipeline is
//! a request/response state machine ([`pipeline::Session`]). `onnxruntime-web`'s
//! `session.run` returns a Promise and Rust in WASM cannot block on one, so the
//! engine emits one [`pipeline::HostStep::Infer`] at a time and waits to be
//! handed the outputs. Native `ort` drives the same loop synchronously.
//!
//! The consequence is that perception and decision logic exist once, compile to
//! both `x86_64` and `wasm32-unknown-unknown`, and a new backend (candle, GGUF,
//! a remote service) is a new `Executor`, not a fork of the pipeline.
//!
//! ## Honesty rules
//!
//! These are load-bearing, not stylistic:
//!
//! - A line below [`params::MIN_CONF`] keeps its box and yields **no text**. On
//!   empty fields, checkboxes and rules the recognizer reads `-`, `I` or `1` at
//!   low confidence, and those readings look exactly like real data.
//! - A model the host did not load causes the stage to be **skipped and noted**,
//!   never simulated. [`pipeline::PageResult::notes`] carries every degradation.
//! - Laya reports a field it cannot validate as invalid or absent. It never
//!   fills a gap with a plausible value.
//! - [`params::Profile::Handwriting`] changes detector and crop geometry only.
//!   Those constants are **candidates to be measured**, not claimed wins; the
//!   reference profile [`params::Profile::Indra`] reproduces the shipped Indra
//!   defaults exactly so the difference is measurable on ground truth.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod clock;
pub mod crop;
pub mod ctc;
pub mod det;
pub mod image;
pub mod laya;
pub mod params;
pub mod pipeline;
pub mod skew;
pub mod tensor;
pub mod vocab;

pub use crop::{crop_line, is_dark_line, pad_batch, width_batches, LineCrop};
pub use ctc::{char_positions, collapse, CtcReading};
pub use det::{decode, merge_fragments, LineBox};
pub use image::{page_to_input, Gray8, InkMap};
pub use laya::{ConfidenceBand, Extraction, ExtractedField, ScriptGroup};
pub use params::{CropParams, DetectParams, EngineConfig, Profile};
pub use pipeline::{HostStep, LineResult, PageResult, Session, Stage, StageTimings};
pub use tensor::{DType, EngineError, Executor, InferRequest, ModelKind, NamedTensor, Tensor, TensorData};
pub use vocab::{Meta, Vocab};

/// Engine version, surfaced in the UI provenance strip and `--version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The model set NAYEN v1 was built and tested against.
///
/// Pinned because a tag's files never change: a silent weight swap would alter
/// every number this engine reports.
pub const MODEL_REPO: &str = "datalab-to/indra-ocr";
pub const MODEL_REVISION: &str = "v1.1.0";
/// The models' licence, which the host must surface to the operator.
pub const MODEL_LICENSE: &str = "openrail";

/// The minimum model set for the Latin handwriting path.
///
/// `det.onnx` (0.37 MB) + `rec.onnx` (4.81 MB) + `meta.json`. Everything else
/// degrades gracefully: no orientation correction, no multi-script routing, no
/// reading-order model.
pub const CORE_MODELS: [ModelKind; 2] = [ModelKind::Det, ModelKind::Rec];

/// The full fast path, still well under 10 MB.
pub const RECOMMENDED_MODELS: [ModelKind; 4] = [
    ModelKind::Orient,
    ModelKind::Det,
    ModelKind::Script,
    ModelKind::Rec,
];

/// Build the vocabulary map a [`Session`] needs from a parsed `meta.json`.
///
/// Only the base vocabulary is required; group vocabularies are added when
/// present, and a group recognizer whose vocabulary is missing falls back to
/// base with a note rather than decoding ids against the wrong table.
pub fn vocabs_from_meta(meta: &Meta) -> Result<std::collections::BTreeMap<&'static str, Vocab>, vocab::VocabError> {
    let mut m = std::collections::BTreeMap::new();
    m.insert("base", meta.base_vocab()?);
    for (key, field) in [
        ("world", meta.vocab_world.as_deref()),
        ("other", meta.vocab_world.as_deref()),
        ("cjk", meta.vocab_cjk.as_deref()),
        ("rtl", meta.vocab_rtl.as_deref()),
    ] {
        if let Some(s) = field {
            if let Ok(v) = Vocab::from_json_str(s) {
                m.insert(key, v);
            }
        }
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_model_set_is_the_fast_path() {
        assert_eq!(CORE_MODELS.len(), 2);
        assert!(CORE_MODELS.iter().all(|m| m.is_core()));
        assert!(RECOMMENDED_MODELS.contains(&ModelKind::Orient));
    }

    #[test]
    fn model_provenance_is_pinned() {
        assert_eq!(MODEL_REPO, "datalab-to/indra-ocr");
        assert!(!MODEL_REVISION.is_empty());
        assert_eq!(MODEL_LICENSE, "openrail");
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn vocabs_from_meta_requires_base_and_tolerates_missing_groups() {
        let meta = Meta::parse(
            r#"{"det_long_side":1024,"rec_height":32,"rec_window":3,
                "vocab":"{\"blank\":0,\"chars\":[\"a\",\"b\"]}",
                "vocab_cjk":"{\"blank\":0,\"chars\":[\"中\"]}"}"#,
        )
        .unwrap();
        let v = vocabs_from_meta(&meta).unwrap();
        assert!(v.contains_key("base"));
        assert!(v.contains_key("cjk"));
        assert!(!v.contains_key("rtl"), "absent group vocabulary is simply absent");

        let broken = Meta::parse(
            r#"{"det_long_side":1024,"rec_height":32,"rec_window":3,"vocab":"{\"blank\":0,\"chars\":[]}"}"#,
        )
        .unwrap();
        assert!(vocabs_from_meta(&broken).is_err(), "an unusable base vocab must fail loudly");
    }

    #[test]
    fn engine_config_defaults_are_the_reference_profile() {
        let c = EngineConfig::default();
        assert_eq!(c.profile, Profile::Indra);
        assert_eq!(c.detect, DetectParams::indra());
        assert!(c.orient && c.deskew && c.route_scripts && c.laya);
    }
}
