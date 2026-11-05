//! Recognizer vocabulary, from `meta.json`.
//!
//! Class 0 is the CTC blank; class `i + 1` is `chars[i]`. Every recognizer in
//! the family carries its own vocabulary, and a group recognizer's ids mean
//! nothing in the base vocabulary — so each is decoded with the table it was
//! trained against. Mixing them silently produces garbage rather than an error,
//! which is why the vocab travels with the session that produced the ids.

use serde::{Deserialize, Serialize};

/// The JSON shape inside `meta.json`'s `vocab` field. It is a *string* holding
/// JSON, so it is parsed twice — matching `Vocab.from_json(meta["vocab"])`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VocabJson {
    pub blank: i64,
    pub chars: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vocab {
    pub blank: i64,
    pub chars: Vec<String>,
}

impl Vocab {
    /// Parse the inner JSON string.
    pub fn from_json_str(s: &str) -> Result<Self, VocabError> {
        let v: VocabJson = serde_json::from_str(s).map_err(|e| VocabError::Parse(e.to_string()))?;
        Self::from_parts(v.blank, v.chars)
    }

    pub fn from_parts(blank: i64, chars: Vec<String>) -> Result<Self, VocabError> {
        if chars.is_empty() {
            return Err(VocabError::Empty);
        }
        if blank != 0 {
            return Err(VocabError::UnexpectedBlank(blank));
        }
        // Multi-byte tokens (a LaTeX command like `\frac`) are legitimate, but an
        // empty token would silently swallow a class id.
        if chars.iter().any(|c| c.is_empty()) {
            return Err(VocabError::EmptyToken);
        }
        Ok(Self { blank, chars })
    }

    /// `Vocab.decode`: map class ids to characters, dropping blank (0) and any
    /// id outside the table. An out-of-range id is skipped, never wrapped —
    /// wrapping would turn a model-version mismatch into plausible wrong text.
    pub fn decode(&self, ids: &[i64]) -> String {
        let mut out = String::with_capacity(ids.len() * 2);
        for &i in ids {
            if i > 0 && (i as usize) <= self.chars.len() {
                out.push_str(&self.chars[(i - 1) as usize]);
            }
        }
        out
    }

    /// A single class id, for building per-character overlays.
    pub fn token(&self, id: i64) -> Option<&str> {
        if id > 0 && (id as usize) <= self.chars.len() {
            Some(&self.chars[(id - 1) as usize])
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
}

/// The full `meta.json`, restricted to the fields the engine reads. Unknown
/// fields are ignored so a newer model set does not break an older engine.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Meta {
    pub det_long_side: usize,
    pub rec_height: usize,
    /// `rec.onnx` is fed a `window`-tall crop; 3 gives the 96px input.
    #[serde(default = "default_window")]
    pub rec_window: usize,
    /// Vocabulary JSON *strings*, parsed on demand.
    pub vocab: String,
    #[serde(default)]
    pub vocab_world: Option<String>,
    #[serde(default)]
    pub vocab_cjk: Option<String>,
    #[serde(default)]
    pub vocab_rtl: Option<String>,
    /// Script class names in `script.onnx` output order.
    #[serde(default)]
    pub script_groups: Option<Vec<String>>,
}

fn default_window() -> usize {
    3
}

impl Meta {
    pub fn parse(json: &str) -> Result<Self, VocabError> {
        serde_json::from_str(json).map_err(|e| VocabError::Parse(e.to_string()))
    }

    pub fn base_vocab(&self) -> Result<Vocab, VocabError> {
        Vocab::from_json_str(&self.vocab)
    }

    pub fn vocab_for(&self, group: &str) -> Option<Result<Vocab, VocabError>> {
        let s = match group {
            "base" => Some(self.vocab.as_str()),
            "world" | "other" => self.vocab_world.as_deref(),
            "cjk" => self.vocab_cjk.as_deref(),
            "rtl" => self.vocab_rtl.as_deref(),
            _ => None,
        };
        s.map(Vocab::from_json_str)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VocabError {
    Parse(String),
    Empty,
    EmptyToken,
    UnexpectedBlank(i64),
}

impl std::fmt::Display for VocabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "vocab JSON: {e}"),
            Self::Empty => write!(f, "vocab has no characters"),
            Self::EmptyToken => write!(f, "vocab contains an empty token"),
            Self::UnexpectedBlank(b) => write!(f, "expected CTC blank at class 0, got {b}"),
        }
    }
}

impl std::error::Error for VocabError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vocab {
        Vocab::from_json_str(r#"{"blank": 0, "chars": ["a", "b", "c", " "]}"#).unwrap()
    }

    #[test]
    fn class_zero_is_blank_and_is_dropped() {
        let v = sample();
        assert_eq!(v.decode(&[0, 1, 0, 2]), "ab");
    }

    #[test]
    fn out_of_range_ids_are_skipped_not_wrapped() {
        let v = sample();
        assert_eq!(v.decode(&[1, 99, -3, 2]), "ab");
        assert_eq!(v.token(99), None);
        assert_eq!(v.token(1), Some("a"));
    }

    #[test]
    fn empty_vocab_is_rejected() {
        assert!(matches!(
            Vocab::from_json_str(r#"{"blank":0,"chars":[]}"#),
            Err(VocabError::Empty)
        ));
    }

    #[test]
    fn nonzero_blank_is_rejected() {
        assert!(matches!(
            Vocab::from_json_str(r#"{"blank":1,"chars":["a"]}"#),
            Err(VocabError::UnexpectedBlank(1))
        ));
    }

    #[test]
    fn parses_the_real_meta_shape() {
        // Mirrors datalab-to/indra-ocr meta.json: vocab is a JSON *string*.
        let meta = Meta::parse(
            r#"{"det_long_side":1024,"rec_height":32,"rec_window":3,
                "vocab":"{\"blank\":0,\"chars\":[\"a\",\"b\"]}",
                "vocab_world":"{\"blank\":0,\"chars\":[\"x\"]}",
                "script_groups":["base","cjk","rtl","other"]}"#,
        )
        .unwrap();
        assert_eq!(meta.det_long_side, 1024);
        assert_eq!(meta.rec_window, 3);
        assert_eq!(meta.base_vocab().unwrap().chars, vec!["a", "b"]);
        assert!(meta.vocab_for("cjk").is_none(), "no vocab_cjk in this meta");
        assert_eq!(
            meta.vocab_for("world").unwrap().unwrap().chars,
            vec!["x"]
        );
    }

    #[test]
    fn rec_window_defaults_to_three() {
        let meta =
            Meta::parse(r#"{"det_long_side":1024,"rec_height":32,"vocab":"{\"blank\":0,\"chars\":[\"a\"]}"}"#)
                .unwrap();
        assert_eq!(meta.rec_window, 3);
    }
}
