//! `nayen-wasm` — the browser half of the NAYEN v1 adapter seam.
//!
//! Exposes the request/response state machine in `nayen-core` to JavaScript
//! through the surface declared in `engine/src/types.ts`. Tensor payloads never
//! travel through JSON: they live in a scratch buffer in WASM linear memory and
//! cross the boundary as `{dtype, shape, offset, length}` descriptors.
//!
//! ## Scratch protocol
//!
//! `offset` is relative to `nayen_scratch_ptr()` and `length` is an element
//! count (bytes = length x dtype width). Ownership alternates:
//!
//! 1. The host writes RGBA at offset 0 and grows scratch to fit it.
//! 2. The engine copies the pixels out, resets scratch to zero, appends each
//!    request's *input* tensors, and reports their offsets in the `HostStep`.
//! 3. The host copies those inputs out, runs the model, appends the *outputs*
//!    after `nayen_scratch_len()`, and reports their offsets to `nayen_feed`.
//! 4. `nayen_feed` reads the outputs, resets the length to zero, and writes the
//!    next request's inputs from offset 0.
//!
//! Step 4 is safe because the host has already copied the inputs out and
//! computes its next output cursor from `nayen_scratch_len()` afresh. It bounds
//! scratch to roughly one step's traffic instead of one page's, which matters
//! because a 1024px detector input alone is 4 MB of f32.
//!
//! Capacity is retained across resets, and `nayen_scratch_ptr()` may move when
//! the buffer grows — the host re-queries it before every view.

use std::cell::RefCell;
use std::collections::BTreeMap;

use nayen_core::pipeline::{HostStep, Session};
use nayen_core::tensor::{EngineError, ModelKind, Tensor, TensorData};
use nayen_core::vocab::Meta;
use nayen_core::{EngineConfig, Profile};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

thread_local! {
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static SESSIONS: RefCell<BTreeMap<u32, SessionState>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT_ID: RefCell<u32> = const { RefCell::new(1) };
    static META: RefCell<Option<Meta>> = const { RefCell::new(None) };
}

/// A session slot. `Session` cannot exist without page pixels, so the slot holds
/// the resolved config until `nayen_start` supplies them. One enum for both
/// states means `feed` before `start` is a reported protocol error rather than a
/// quietly-constructed placeholder being inferred on.
enum SessionState {
    Pending {
        cfg: EngineConfig,
        available: Vec<ModelKind>,
    },
    Live(Session),
}

/* ------------------------------------------------------------------- dtos */

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum DTypeDto {
    F32,
    I64,
    Bool,
}

impl From<nayen_core::DType> for DTypeDto {
    fn from(d: nayen_core::DType) -> Self {
        match d {
            nayen_core::DType::F32 => Self::F32,
            nayen_core::DType::I64 => Self::I64,
            nayen_core::DType::Bool => Self::Bool,
        }
    }
}

fn dtype_bytes(d: DTypeDto) -> usize {
    match d {
        DTypeDto::F32 => 4,
        DTypeDto::I64 => 8,
        DTypeDto::Bool => 1,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TensorDto {
    dtype: DTypeDto,
    shape: Vec<i64>,
    offset: usize,
    length: usize,
}

#[derive(Debug, Serialize)]
struct NamedTensorDto {
    name: String,
    tensor: TensorDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestDto {
    model: ModelKind,
    inputs: Vec<NamedTensorDto>,
    outputs: Vec<String>,
}

/// The `HostStep` union from `engine/src/types.ts`.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum StepDto {
    Infer {
        request: RequestDto,
    },
    Done {
        result: nayen_core::PageResult,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Deserialize)]
struct NamedTensorIn {
    #[allow(dead_code)]
    name: String,
    tensor: TensorIn,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TensorIn {
    dtype: DTypeDto,
    shape: Vec<i64>,
    offset: usize,
    length: usize,
}

/// `EngineConfig` from `engine/src/types.ts`, field for field. Detection and
/// crop geometry are not here on purpose: they come from the chosen `Profile`,
/// which is the point of having profiles.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsEngineConfig {
    profile: Profile,
    min_conf: f32,
    orient: bool,
    route_scripts: bool,
    laya: bool,
    max_page_side: usize,
}

/// The `NayenInit` payload the host hands to [`nayen_create`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitRequest {
    config: JsEngineConfig,
    /// Models the host can actually serve. Anything absent is skipped with a
    /// note; the engine never fabricates a result for a missing model.
    available: Vec<ModelKind>,
    /// Raw `meta.json` text, for the recognizer vocabularies. Without it no text
    /// can be decoded, so it is required rather than defaulted.
    meta: String,
    /// Host-specific, so it lives on the init request rather than in the shared
    /// config type: the deskew search is ~90 thumbnail rotations, which is
    /// noticeable on a phone. Off unless the host asks for it.
    #[serde(default)]
    deskew: bool,
}

fn build_config(init: &InitRequest) -> EngineConfig {
    let js = &init.config;
    EngineConfig {
        min_conf: js.min_conf,
        orient: js.orient,
        route_scripts: js.route_scripts,
        laya: js.laya,
        max_page_side: js.max_page_side,
        deskew: init.deskew,
        ..EngineConfig::browser(js.profile)
    }
}

/* -------------------------------------------------------------- plumbing */

fn err(code: &str, message: impl Into<String>) -> String {
    serde_json::to_string(&StepDto::Error {
        code: code.to_string(),
        message: message.into(),
    })
    .unwrap_or_else(|_| {
        r#"{"kind":"error","code":"serialize","message":"unserializable error"}"#.to_string()
    })
}

fn engine_err(e: &EngineError) -> String {
    let code = match e {
        EngineError::ModelUnavailable(_) => "model_unavailable",
        EngineError::ShapeMismatch { .. } => "shape_mismatch",
        EngineError::Image(_) => "image",
        EngineError::Decode(_) => "decode",
        EngineError::Host(_) => "host",
    };
    err(code, e.to_string())
}

/// Append a tensor's payload to scratch and describe it.
fn push_tensor(scratch: &mut Vec<u8>, t: &Tensor) -> TensorDto {
    let offset = scratch.len();
    let (dtype, length) = match &t.data {
        TensorData::F32(v) => {
            scratch.reserve(v.len() * 4);
            for &x in v {
                scratch.extend_from_slice(&x.to_le_bytes());
            }
            (DTypeDto::F32, v.len())
        }
        TensorData::I64(v) => {
            scratch.reserve(v.len() * 8);
            for &x in v {
                scratch.extend_from_slice(&x.to_le_bytes());
            }
            (DTypeDto::I64, v.len())
        }
        TensorData::Bool(v) => {
            scratch.extend(v.iter().map(|&b| u8::from(b)));
            (DTypeDto::Bool, v.len())
        }
    };
    TensorDto {
        dtype,
        shape: t.shape.clone(),
        offset,
        length,
    }
}

/// Serialize a step, laying any input tensors into scratch as it goes.
fn step_to_json(step: HostStep) -> String {
    match step {
        HostStep::Done(result) => {
            SCRATCH.with(|s| s.borrow_mut().clear());
            serde_json::to_string(&StepDto::Done { result: *result })
        }
        HostStep::Failed(e) => return engine_err(&e),
        HostStep::Infer(req) => {
            let dto = SCRATCH.with(|s| {
                let mut scratch = s.borrow_mut();
                let inputs = req
                    .inputs
                    .iter()
                    .map(|nt| NamedTensorDto {
                        name: nt.name.clone(),
                        tensor: push_tensor(&mut scratch, &nt.tensor),
                    })
                    .collect();
                RequestDto {
                    model: req.model,
                    inputs,
                    outputs: req.outputs,
                }
            });
            serde_json::to_string(&StepDto::Infer { request: dto })
        }
    }
    .unwrap_or_else(|e| err("serialize", format!("could not serialize the engine step: {e}")))
}

/// Read one described tensor back out of scratch.
fn read_tensor(scratch: &[u8], t: &TensorIn) -> Result<Tensor, String> {
    let bytes = t
        .length
        .checked_mul(dtype_bytes(t.dtype))
        .ok_or("tensor length overflow")?;
    let end = t.offset.checked_add(bytes).ok_or("tensor offset overflow")?;
    if end > scratch.len() {
        return Err(format!(
            "tensor at offset {} + {} bytes exceeds the {} byte scratch buffer",
            t.offset,
            bytes,
            scratch.len()
        ));
    }
    let raw = &scratch[t.offset..end];
    Ok(match t.dtype {
        DTypeDto::F32 => {
            if raw.len() % 4 != 0 {
                return Err("f32 tensor byte length is not a multiple of 4".into());
            }
            let v: Vec<f32> = raw
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            Tensor::f32(t.shape.clone(), v)
        }
        DTypeDto::I64 => {
            if raw.len() % 8 != 0 {
                return Err("i64 tensor byte length is not a multiple of 8".into());
            }
            let v: Vec<i64> = raw
                .chunks_exact(8)
                .map(|c| {
                    i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
                })
                .collect();
            Tensor::i64(t.shape.clone(), v)
        }
        DTypeDto::Bool => Tensor::bool_t(t.shape.clone(), raw.iter().map(|&b| b != 0).collect()),
    })
}

/* ---------------------------------------------------------------- exports */

/// Engine version and model provenance, for the UI's telemetry strip.
#[wasm_bindgen]
pub fn nayen_version() -> String {
    format!(
        "nayen-core {} | models {}@{} ({})",
        nayen_core::VERSION,
        nayen_core::MODEL_REPO,
        nayen_core::MODEL_REVISION,
        nayen_core::MODEL_LICENSE
    )
}

#[wasm_bindgen]
pub fn nayen_scratch_ptr() -> usize {
    SCRATCH.with(|s| s.borrow().as_ptr() as usize)
}

#[wasm_bindgen]
pub fn nayen_scratch_len() -> usize {
    SCRATCH.with(|s| s.borrow().len())
}

/// Grow scratch to at least `bytes`. Returns the new length, or 0 on failure.
#[wasm_bindgen]
pub fn nayen_scratch_grow(bytes: usize) -> usize {
    SCRATCH.with(|s| {
        let mut scratch = s.borrow_mut();
        if bytes > scratch.len() {
            scratch.resize(bytes, 0);
        }
        scratch.len()
    })
}

/// Create a session from the `NayenInit` JSON `{config, available, meta, deskew?}`.
///
/// Returns the session id, or 0 with the reason on `console.error`. Deliberately
/// does **not** touch the scratch buffer: the host writes the page RGBA there
/// before calling this.
#[wasm_bindgen]
pub fn nayen_create(init_json: &str) -> i32 {
    // Idempotent, and cheap enough to not be worth guarding.
    nayen_core::clock::set_clock(js_clock::ms);

    let init: InitRequest = match serde_json::from_str(init_json) {
        Ok(v) => v,
        Err(e) => {
            console_error(&format!("nayen_create: bad init JSON: {e}"));
            return 0;
        }
    };

    let meta = match Meta::parse(&init.meta) {
        Ok(m) => m,
        Err(e) => {
            console_error(&format!("nayen_create: could not parse meta.json: {e}"));
            return 0;
        }
    };
    META.with(|slot| *slot.borrow_mut() = Some(meta));

    let state = SessionState::Pending {
        cfg: build_config(&init),
        available: init.available,
    };
    let id = NEXT_ID.with(|n| {
        let mut next = n.borrow_mut();
        let id = *next;
        *next += 1;
        id
    });
    SESSIONS.with(|s| s.borrow_mut().insert(id, state));
    id as i32
}

/// Begin a page. `rgba_offset` is relative to `nayen_scratch_ptr()`.
#[wasm_bindgen]
pub fn nayen_start(session: i32, rgba_offset: usize, width: usize, height: usize) -> String {
    if session <= 0 {
        return err("session", "invalid session id");
    }
    let id = session as u32;

    let pending = SESSIONS.with(|s| match s.borrow_mut().remove(&id) {
        Some(SessionState::Pending { cfg, available }) => Ok((cfg, available)),
        Some(SessionState::Live(_)) => Err("nayen_start called twice on the same session"),
        None => Err("no such session; call nayen_create first"),
    });
    let (cfg, available) = match pending {
        Ok(v) => v,
        Err(e) => return err("protocol", e),
    };

    let meta = META.with(|m| m.borrow().clone());
    let Some(meta) = meta else {
        return err(
            "meta",
            "meta.json was not accepted by nayen_create; the recognizer vocabulary is unknown",
        );
    };
    let vocabs = match nayen_core::vocabs_from_meta(&meta) {
        Ok(v) => v,
        Err(e) => return err("vocab", e.to_string()),
    };

    // Copy the page out and release the pixels: the engine never reads scratch
    // again, so resetting here keeps the first input tensor at offset 0 instead
    // of stranding megabytes of RGBA ahead of it.
    let rgba = SCRATCH.with(|s| {
        let scratch = s.borrow();
        let bytes = width
            .checked_mul(height)
            .and_then(|px| px.checked_mul(4))
            .ok_or("page dimensions overflow")?;
        let end = rgba_offset
            .checked_add(bytes)
            .ok_or("page offset overflow")?;
        if end > scratch.len() {
            return Err(format!(
                "page RGBA needs {bytes} bytes at offset {rgba_offset} but scratch holds {}",
                scratch.len()
            ));
        }
        Ok::<Vec<u8>, String>(scratch[rgba_offset..end].to_vec())
    });
    let rgba = match rgba {
        Ok(v) => v,
        Err(e) => return err("scratch", e),
    };
    SCRATCH.with(|s| s.borrow_mut().clear());

    let mut sess = match Session::new(cfg, &rgba, width, height, vocabs, available) {
        Ok(s) => s,
        Err(e) => return engine_err(&e),
    };
    let step = sess.start();
    SESSIONS.with(|s| s.borrow_mut().insert(id, SessionState::Live(sess)));
    step_to_json(step)
}

/// Hand the outputs of the outstanding request back to the engine.
#[wasm_bindgen]
pub fn nayen_feed(session: i32, outputs_json: &str) -> String {
    if session <= 0 {
        return err("session", "invalid session id");
    }
    let id = session as u32;

    let described: Vec<NamedTensorIn> = match serde_json::from_str(outputs_json) {
        Ok(v) => v,
        Err(e) => return err("protocol", format!("bad outputs JSON: {e}")),
    };

    // Read every payload out of scratch before touching the session, so the two
    // borrows never overlap.
    let outputs = SCRATCH.with(|s| {
        let scratch = s.borrow();
        described
            .iter()
            .map(|nt| read_tensor(&scratch, &nt.tensor))
            .collect::<Result<Vec<_>, _>>()
    });
    let outputs = match outputs {
        Ok(v) => v,
        Err(e) => return err("scratch", e),
    };

    let step = SESSIONS.with(|s| {
        let mut map = s.borrow_mut();
        match map.get_mut(&id) {
            Some(SessionState::Live(sess)) => {
                // Step 4 of the scratch protocol: the host has copied its inputs
                // out and recomputes its output cursor from scratch_len(), so
                // resetting here bounds scratch to one step of traffic.
                SCRATCH.with(|sc| sc.borrow_mut().clear());
                sess.feed(outputs)
            }
            Some(SessionState::Pending { .. }) => HostStep::Failed(EngineError::Host(format!(
                "session {id} has not been started; call nayen_start before nayen_feed"
            ))),
            None => HostStep::Failed(EngineError::Host(format!("no session {id}"))),
        }
    });
    step_to_json(step)
}

#[wasm_bindgen]
pub fn nayen_destroy(session: i32) {
    if session <= 0 {
        return;
    }
    let id = session as u32;
    SESSIONS.with(|s| s.borrow_mut().remove(&id));
    SCRATCH.with(|s| s.borrow_mut().clear());
}

/// A one-function shim so this crate does not pull in `web-sys` purely to log.
mod js_console {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console, js_name = error)]
        fn js_error(msg: &str);
    }

    pub fn error(msg: &str) {
        js_error(msg);
    }
}

fn console_error(msg: &str) {
    js_console::error(msg);
}

/// The engine clock. `std::time::Instant` is a stub on `wasm32-unknown-unknown`
/// — it compiles and always reports zero elapsed — so every stage timing would
/// come back as `0.0 ms`, which reads as "instant" instead of "not measured".
/// `performance.now()` is sub-millisecond and exists in both a window and a
/// worker; installing it makes the browser timings as real as the CLI's.
mod js_clock {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = ["self", "performance"], js_name = now)]
        fn now() -> f64;
    }

    pub fn ms() -> f64 {
        now()
    }
}
