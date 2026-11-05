//! The native `ort` executor.
//!
//! This is the desktop half of the adapter seam: `nayen-core` asks for tensors,
//! this runs them through ONNX Runtime and hands the results back. The browser
//! half does the same over `onnxruntime-web`. Neither knows about the other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use nayen_core::tensor::{EngineError, Executor, InferRequest, ModelKind, Tensor, TensorData};
use ort::session::{Session, SessionInputValue};
use ort::value::{DynValue, Tensor as OrtTensor};

/// ONNX Runtime sessions keyed by the model the engine asks for.
///
/// Loading is explicit: a model file that is absent is simply not in the map,
/// so the engine skips that stage and records a note. Nothing here falls back to
/// a placeholder, because a placeholder that returns zeros would silently
/// fabricate OCR output.
pub struct OrtExecutor {
    sessions: BTreeMap<ModelKind, Session>,
    loaded: Vec<ModelKind>,
}

/// Locate the ONNX Runtime shared library `ort` should dlopen.
///
/// `ORT_DYLIB_PATH` wins outright. Otherwise look for `var/onnxruntime` — what
/// `scripts/fetch_ort` writes — from the working directory up to the filesystem
/// root, so `cargo run -p nayen-cli` and `cargo test`, which both start inside a
/// crate directory, still find a runtime installed at the repository root.
/// Finally look next to the running executable, which is how the Windows package
/// ships the DLL. The library is never downloaded implicitly: a missing runtime
/// is an error naming the script that fetches it, not a silent fallback to a stub.
fn resolve_dylib() -> Result<PathBuf> {
    const NAMES: &[&str] = &["libonnxruntime.so", "onnxruntime.dll", "libonnxruntime.dylib"];

    if let Ok(explicit) = std::env::var("ORT_DYLIB_PATH") {
        let path = PathBuf::from(&explicit);
        return path.is_file().then_some(path).ok_or_else(|| {
            anyhow::anyhow!("ORT_DYLIB_PATH={explicit} does not point at a file")
        });
    }

    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.extend(cwd.ancestors().map(|p| p.join("var/onnxruntime")));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("onnxruntime"));
            roots.push(dir.to_path_buf());
        }
    }
    for root in &roots {
        for name in NAMES {
            let candidate = root.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    bail!(
        "no ONNX Runtime shared library found. Fetch one with ./scripts/fetch_ort, or set ORT_DYLIB_PATH."
    )
}

impl OrtExecutor {
    /// Load whichever of `wanted` exist in `dir`.
    ///
    /// Returns the executor plus the list of models that were actually missing,
    /// so the caller can report it rather than discovering it mid-page.
    pub fn load(dir: &Path, wanted: &[ModelKind], threads: usize) -> Result<(Self, Vec<ModelKind>)> {
        if !dir.is_dir() {
            bail!(
                "model directory {} does not exist. Fetch the weights first: ./scripts/fetch_indra_models --download --approve-license openrail",
                dir.display()
            );
        }
        let dylib = resolve_dylib()?;
        // ort reads this lazily at first init, so setting it here is enough —
        // and it must happen before `ort::init()` below.
        std::env::set_var("ORT_DYLIB_PATH", &dylib);

        let env = ort::init()
            .with_name("nayen")
            .build()
            .context("failed to create the ONNX Runtime environment")?;

        let mut sessions = BTreeMap::new();
        let mut loaded = Vec::new();
        let mut missing = Vec::new();

        for kind in wanted {
            let path = dir.join(kind.filename());
            if !path.is_file() {
                missing.push(*kind);
                continue;
            }
            // The builder methods return ort's own Result, so anyhow's Context
            // does not apply; map each step to a message naming the file.
            let mut builder = Session::builder(&env)
                .map_err(|e| anyhow::anyhow!("building a session for {}: {e}", path.display()))?;
            if threads > 0 {
                builder = builder
                    .with_intra_threads(threads)
                    .map_err(|e| anyhow::anyhow!("setting threads for {}: {e}", path.display()))?;
            }
            // The optimization level is deliberately not set. ONNX Runtime
            // enables all graph optimizations by default, and passing ort's
            // Level3 ordinal to an older runtime (1.20.x) is rejected as
            // "graph_optimization_level is not valid" — the enum drifted.
            // commit_from_file takes &mut self, so it cannot chain off the value.
            let session = builder
                .commit_from_file(&path)
                .with_context(|| format!("loading {}", path.display()))?;
            sessions.insert(*kind, session);
            loaded.push(*kind);
        }

        Ok((Self { sessions, loaded }, missing))
    }

    pub fn loaded(&self) -> &[ModelKind] {
        &self.loaded
    }

    /// Report a model's declared inputs and outputs. Used by `nayen inspect` so
    /// the tensor contracts in this repo can be re-verified against the actual
    /// weights rather than against documentation.
    pub fn describe(&mut self, kind: ModelKind) -> Result<(Vec<String>, Vec<String>)> {
        let s = self
            .sessions
            .get_mut(&kind)
            .with_context(|| format!("{} is not loaded", kind.filename()))?;
        let inputs = s
            .inputs()
            .iter()
            .map(|i| format!("{} {:?}", i.name(), i.dtype()))
            .collect();
        let outputs = s
            .outputs()
            .iter()
            .map(|o| format!("{} {:?}", o.name(), o.dtype()))
            .collect();
        Ok((inputs, outputs))
    }
}

impl Executor for OrtExecutor {
    fn run(&mut self, req: &InferRequest) -> Result<Vec<Tensor>, EngineError> {
        let session = self
            .sessions
            .get_mut(&req.model)
            .ok_or_else(|| EngineError::ModelUnavailable(req.model.filename().to_string()))?;

        // Build owned ORT tensors from the engine's request. The data is copied:
        // the engine keeps ownership of its buffers, and inference outlives the
        // borrow in ways that make sharing them fragile.
        let mut inputs: Vec<(std::borrow::Cow<'static, str>, SessionInputValue<'static>)> =
            Vec::with_capacity(req.inputs.len());
        for nt in &req.inputs {
            let value: DynValue = match &nt.tensor.data {
                TensorData::F32(d) => OrtTensor::from_array((nt.tensor.shape.clone(), d.clone()))
                    .map_err(|e| EngineError::Host(format!("building f32 tensor: {e}")))?
                    .into_dyn(),
                TensorData::I64(d) => OrtTensor::from_array((nt.tensor.shape.clone(), d.clone()))
                    .map_err(|e| EngineError::Host(format!("building i64 tensor: {e}")))?
                    .into_dyn(),
                TensorData::Bool(d) => OrtTensor::from_array((nt.tensor.shape.clone(), d.clone()))
                    .map_err(|e| EngineError::Host(format!("building bool tensor: {e}")))?
                    .into_dyn(),
            };
            inputs.push((std::borrow::Cow::Owned(nt.name.clone()), SessionInputValue::Owned(value)));
        }

        let outputs = session
            .run(inputs)
            .map_err(|e| EngineError::Host(format!("{}: {e}", req.model.filename())))?;

        // Return exactly the requested outputs, in the requested order: the
        // engine reads them positionally.
        let mut out = Vec::with_capacity(req.outputs.len());
        for name in &req.outputs {
            let value = outputs
                .get(name.as_str())
                .ok_or_else(|| EngineError::Host(format!("{} produced no output named {name}", req.model.filename())))?;
            out.push(to_core_tensor(value, name, req.model)?);
        }
        Ok(out)
    }

    fn available(&self) -> &[ModelKind] {
        &self.loaded
    }
}

/// Copy an ORT output into the engine's tensor type.
fn to_core_tensor(value: &DynValue, name: &str, kind: ModelKind) -> Result<Tensor, EngineError> {
    if let Ok((shape, data)) = value.try_extract_tensor::<f32>() {
        return Ok(Tensor::f32(shape_vec(shape), data.to_vec()));
    }
    if let Ok((shape, data)) = value.try_extract_tensor::<i64>() {
        return Ok(Tensor::i64(shape_vec(shape), data.to_vec()));
    }
    if let Ok((shape, data)) = value.try_extract_tensor::<bool>() {
        return Ok(Tensor::bool_t(shape_vec(shape), data.to_vec()));
    }
    // An int32 output would land here; models in this family do not emit one,
    // and guessing would corrupt the decode.
    Err(EngineError::ShapeMismatch {
        expected: "float32, int64 or bool".into(),
        got: format!("unsupported dtype for {} output {name}", kind.filename()),
    })
}

fn shape_vec(shape: &ort::value::Shape) -> Vec<i64> {
    shape.iter().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_directory_is_a_clear_error() {
        let e = OrtExecutor::load(Path::new("/nonexistent/nayen-models"), &[ModelKind::Det], 1)
            .err()
            .expect("a missing weights directory must not load");
        let msg = format!("{:#}", e);
        assert!(msg.contains("does not exist"), "{msg}");
        assert!(msg.contains("fetch_indra_models"), "must point at the remedy: {msg}");
    }

    #[test]
    fn executor_with_no_sessions_reports_nothing_available() {
        // An executor built over an empty directory must not claim any model.
        let dir = std::env::temp_dir().join("nayen-empty-models-test");
        std::fs::create_dir_all(&dir).unwrap();
        let (exec, missing) = OrtExecutor::load(&dir, &[ModelKind::Det, ModelKind::Rec], 1).unwrap();
        assert_eq!(missing, vec![ModelKind::Det, ModelKind::Rec]);
        assert!(exec.available().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn running_an_unloaded_model_fails_loudly() {
        let dir = std::env::temp_dir().join("nayen-empty-models-test2");
        std::fs::create_dir_all(&dir).unwrap();
        let (mut exec, _) = OrtExecutor::load(&dir, &[], 1).unwrap();
        let req = InferRequest {
            model: ModelKind::Det,
            inputs: vec![],
            outputs: vec!["out".into()],
        };
        let e = exec.run(&req).unwrap_err();
        assert!(matches!(e, EngineError::ModelUnavailable(_)), "{e:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
