# Rust crate API — `nayen-core`

`nayen-core` is the executor-agnostic engine: perception (Indra) and decision
(Laya) in one crate that compiles for desktop and for
`wasm32-unknown-unknown`. `unsafe_code = "forbid"` crate-wide.

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[features]
default = []
# Emit extra per-stage timings into PageResult. Costs nothing at runtime when off.
instrument = []
```

No inference runtime is linked: `ort`, `onnxruntime-web` and (later)
candle/GGUF all sit behind the `Executor` trait, so hosts own model loading,
memory and device policy.

## Modules

| Module | Responsibility |
| --- | --- |
| `params` | Tunable stage parameters, `Profile`, `DetectParams`, `EngineConfig` and the structural pipeline constants. |
| `tensor` | `DType`, `Tensor`, `InferRequest`, the `Executor` trait and `EngineError`. |
| `image` | `Gray8` page buffers and the resampling rules the Indra models expect. Every stage works on one ink map where `0.0` is paper and `1.0` is ink; a page is normalised once and never re-derived. |
| `det` | LineNet output decoding (`det_post.decode`) plus the box-merging rules the multi-scale sweep needs. |
| `crop` | Line cropping to recognizer geometry (`LINE_HEIGHT = 32`, widths padded to `WIDTH_MULTIPLE = 8`, capped at `MAX_CROP_WIDTH = 4096`). |
| `ctc` | CTC decode and the mean-peak confidence used for `minConf`. |
| `skew` | Bounded skew search on thumbnails (`SKEW_SIDE = 800`, range ±45°, coarse step 2°, fine step 0.1°, floor 0.3°). |
| `vocab` | Recognizer vocabularies and character maps. |
| `laya` | The decision layer: script groups, field rules, CAR/LAR cross-check, anomalies. |

## The `Executor` trait

```rust
pub trait Executor {
    fn run(&mut self, req: &InferRequest) -> Result<Vec<Tensor>, EngineError>;
    fn available(&self) -> &[ModelKind];
}
```

`available()` declares which models the backend can actually serve, so the
engine can skip the orientation or script stages on a host that only loaded
the core detector/recognizer pair instead of failing the page.

## Errors

```rust
pub enum EngineError {
    ModelUnavailable(String),
    ShapeMismatch { expected: String, got: String },
    Image(String),
    Decode(String),
    Host(String),
}
```

Errors are values, not panics. A host that cannot serve a model must surface
`ModelUnavailable` as failure evidence; nothing downstream substitutes
invented text.

## Profiles and configuration

`Profile` selects detection/crop presets; recognizer weights are never
touched by a profile switch:

```rust
pub enum Profile { Indra, Handwriting, Sweep }

impl DetectParams {
    pub const fn indra() -> Self;        // bit-for-bit the reference pipeline
    pub const fn handwriting() -> Self;  // faint-stroke candidates, not proven wins
    pub const fn sweep() -> Self;        // multi-scale detect + merge, opt-in cost
}
```

`EngineConfig` constructors: `EngineConfig::indra()` (shipped defaults),
`EngineConfig::from_profile(profile)`, and `EngineConfig::browser(profile)`
which bounds `maxPageSide` for WASM heap use. Full parameter tables and the
honest “candidates, not proven wins” stance:
[profiles and parameters](sdk-profiles.html).

## Structural constants (not profile-tunable)

These four are structural — changing them desynchronises decode from how the
models were trained:

```rust
pub const CORE_FRAC: f32 = 0.4;
pub const CORE_X_SHRINK: f32 = 0.15;
pub const OUT_STRIDE: usize = 2;
pub const INPUT_MULTIPLE: usize = 32;
```

## Sibling crates

- `nayen-wasm` — the WASM export surface (`nayen_create` / `nayen_start` /
  `nayen_feed` / …) implementing the request/response half of the
  [host protocol](sdk-host-protocol.html). Scaffolded; being filled in.
- `nayen-cli` — native driver looping synchronously over `ort`, plus `eval`
  for profile comparison against ground truth. Scaffolded; being filled in.

Until those crates ship, the Studio and the Windows bundle run the local
Python backend's handwriting adapter through the same evidence contract, and
report “engine not loaded” everywhere else. That state is shown explicitly in
the UI rather than approximated.
