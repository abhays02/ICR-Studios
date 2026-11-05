//! Tensors and the executor seam.
//!
//! NAYEN v1 never links an inference runtime into its core. `ort` on the
//! desktop, `onnxruntime-web` in the browser and (later) candle/GGUF all sit
//! behind [`Executor`], so the perception and decision code is one
//! implementation that compiles unchanged to `wasm32-unknown-unknown`.

use serde::{Deserialize, Serialize};

/// Element types the Indra models actually exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DType {
    F32,
    I64,
    Bool,
}

impl DType {
    pub fn onnx_name(&self) -> &'static str {
        match self {
            Self::F32 => "tensor(float)",
            Self::I64 => "tensor(int64)",
            Self::Bool => "tensor(bool)",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TensorData {
    F32(Vec<f32>),
    I64(Vec<i64>),
    Bool(Vec<bool>),
}

impl TensorData {
    pub fn dtype(&self) -> DType {
        match self {
            Self::F32(_) => DType::F32,
            Self::I64(_) => DType::I64,
            Self::Bool(_) => DType::Bool,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::F32(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::Bool(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn as_f32(&self) -> Option<&[f32]> {
        match self {
            Self::F32(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<&[i64]> {
        match self {
            Self::I64(v) => Some(v),
            _ => None,
        }
    }
}

/// A dense tensor with an explicit shape. `shape` may hold `-1` for a dynamic
/// axis on the way in; the executor resolves it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub shape: Vec<i64>,
    pub data: TensorData,
}

impl Tensor {
    pub fn f32(shape: Vec<i64>, data: Vec<f32>) -> Self {
        Self {
            shape,
            data: TensorData::F32(data),
        }
    }

    pub fn i64(shape: Vec<i64>, data: Vec<i64>) -> Self {
        Self {
            shape,
            data: TensorData::I64(data),
        }
    }

    pub fn bool_t(shape: Vec<i64>, data: Vec<bool>) -> Self {
        Self {
            shape,
            data: TensorData::Bool(data),
        }
    }

    pub fn dtype(&self) -> DType {
        self.data.dtype()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Number of elements implied by `shape`, ignoring dynamic axes.
    pub fn shape_len(&self) -> Option<i64> {
        if self.shape.iter().any(|&d| d < 0) {
            return None;
        }
        Some(self.shape.iter().product())
    }

    /// View `[B, T]` int64 output as row `b`.
    pub fn row_i64(&self, b: usize, t: usize) -> Option<&[i64]> {
        let v = self.data.as_i64()?;
        if self.shape.len() != 2 {
            return None;
        }
        let width = self.shape[1] as usize;
        let start = b * width;
        v.get(start..start + width.min(t))
    }

    /// View `[B, T]` float output as row `b`, up to `t` elements.
    pub fn row_f32(&self, b: usize, t: usize) -> Option<&[f32]> {
        let v = self.data.as_f32()?;
        if self.shape.len() != 2 {
            return None;
        }
        let width = self.shape[1] as usize;
        let start = b * width;
        v.get(start..start + width.min(t))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedTensor {
    pub name: String,
    pub tensor: Tensor,
}

impl NamedTensor {
    pub fn new(name: impl Into<String>, tensor: Tensor) -> Self {
        Self {
            name: name.into(),
            tensor,
        }
    }
}

/// Which weight file a request targets. Hosts map these to their own session
/// handles; a host that lacks one returns [`EngineError::ModelUnavailable`] and
/// the engine degrades instead of inventing output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    /// Page rotation classifier. `[B,1,768,768] -> [B,5]`
    Orient,
    /// LineNet line detector. `[1,1,H,W] -> [1,3,h,w]`
    Det,
    /// Script classifier. `[B,1,32,W] + widths -> [B,4]`
    Script,
    /// Latin CTC recognizer. `[B,1,96,W] + widths -> ids,conf,lengths`
    Rec,
    /// Multi-script CTC recognizer.
    RecWorld,
    /// CJK CTC recognizer.
    RecCjk,
    /// Right-to-left CTC recognizer.
    RecRtl,
    /// Reading-order transformer. `feats,mask -> a,cls,same,tab`
    Order,
}

impl ModelKind {
    pub fn filename(&self) -> &'static str {
        match self {
            Self::Orient => "orient.onnx",
            Self::Det => "det.onnx",
            Self::Script => "script.onnx",
            Self::Rec => "rec.onnx",
            Self::RecWorld => "rec_world.onnx",
            Self::RecCjk => "rec_cjk.onnx",
            Self::RecRtl => "rec_rtl.onnx",
            Self::Order => "order.onnx",
        }
    }

    /// Minimum set for the fast Latin handwriting path.
    pub fn is_core(&self) -> bool {
        matches!(self, Self::Det | Self::Rec)
    }
}

/// One inference call the host must perform.
#[derive(Clone, Debug, PartialEq)]
pub struct InferRequest {
    pub model: ModelKind,
    pub inputs: Vec<NamedTensor>,
    /// Output names, in the order the engine expects them back.
    pub outputs: Vec<String>,
}

/// The seam every backend implements.
///
/// Native `ort` satisfies this synchronously; the browser host satisfies it by
/// awaiting `onnxruntime-web` and calling back into WASM. Because the engine is
/// a request/response state machine, neither side needs to block the other.
pub trait Executor {
    fn run(&mut self, req: &InferRequest) -> Result<Vec<Tensor>, EngineError>;

    /// Which models this backend can actually serve. Lets the engine skip the
    /// orientation or script stages on a host that only loaded the core pair.
    fn available(&self) -> &[ModelKind];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EngineError {
    ModelUnavailable(String),
    ShapeMismatch {
        expected: String,
        got: String,
    },
    Image(String),
    Decode(String),
    Host(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelUnavailable(m) => write!(f, "model unavailable: {m}"),
            Self::ShapeMismatch { expected, got } => {
                write!(f, "tensor shape mismatch: expected {expected}, got {got}")
            }
            Self::Image(e) => write!(f, "image error: {e}"),
            Self::Decode(e) => write!(f, "decode error: {e}"),
            Self::Host(e) => write!(f, "executor error: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<crate::image::ImageError> for EngineError {
    fn from(e: crate::image::ImageError) -> Self {
        Self::Image(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_views_slice_by_shape() {
        let t = Tensor::i64(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(t.row_i64(0, 3), Some([1i64, 2, 3].as_slice()));
        assert_eq!(t.row_i64(1, 2), Some([4i64, 5].as_slice()));
    }

    #[test]
    fn model_filenames_match_indra_repo() {
        assert_eq!(ModelKind::Det.filename(), "det.onnx");
        assert_eq!(ModelKind::RecWorld.filename(), "rec_world.onnx");
        assert!(ModelKind::Det.is_core());
        assert!(!ModelKind::Orient.is_core());
    }

    #[test]
    fn dtype_names_match_onnx() {
        assert_eq!(DType::F32.onnx_name(), "tensor(float)");
        assert_eq!(DType::I64.onnx_name(), "tensor(int64)");
    }
}
