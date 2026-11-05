# Host protocol and the executor seam

The contract between `nayen-core` and every host that drives it lives in
[`engine/src/types.ts`](https://github.com/abhays02/ICR-Studios/blob/main/engine/src/types.ts)
and must stay in sync with `crates/nayen-wasm/src/lib.rs` (the engine's
`check:contract` script diffs the two). Excerpts below are pulled from the
contract file at build time — they are the real definitions, not paraphrases.

## Why a request/response state machine

`onnxruntime-web`'s `session.run()` returns a Promise, and Rust compiled to
WASM cannot block on a JS promise. So the engine never calls the runtime
itself: it emits one `InferRequest` at a time and waits to be handed the
outputs back. The same engine code therefore works natively (a synchronous
loop over `ort`) and in the browser (a driver awaiting `onnxruntime-web`
between steps). Neither host needs threads, atomics, or a WASM-side event loop.

## One inference call

%%include engine/src/types.ts#InferRequest%%

Tensor payloads never travel through JSON. A `TensorRef` points into the WASM
linear-memory scratch buffer the host reads and writes directly:

%%include engine/src/types.ts#TensorRef%%

## What the host gets back

Each step of the loop returns a `HostStep`: either the next `infer` request,
the finished `PageResult`, or a structured `error` — a failure is evidence,
never a silently empty result:

%%include engine/src/types.ts#HostStep%%

## Page result

The engine's complete answer for one page. Note `timings` (per-stage
wall-clock milliseconds measured **inside** the engine), `profile`, and
`modelsUsed` — the UI provenance strip shows only models the engine actually
used:

%%include engine/src/types.ts#PageResult%%

Lines whose mean CTC confidence falls below `minConf` carry an **empty**
`text` — the box is kept as layout evidence, but the engine never guesses:

%%include engine/src/types.ts#LineResult%%

## Laya's structured read

%%include engine/src/types.ts#FieldExtraction%%

`ExtractedField.valid` is `null` when the field was not found: absence is
reported, not filled in. `carLarMatch` cross-checks amount-in-words against
amount-in-figures.

## Configuration

%%include engine/src/types.ts#EngineConfig%%

Defaults shipped with the contract:

%%include engine/src/types.ts#DEFAULT_CONFIG%%

## Progress events

Hosts surface driver-loop progress with one event per stage transition —
these are real transitions, not synthetic percentages:

%%include engine/src/types.ts#ProgressEvent%%

## Failure semantics

- A missing weight file or an unavailable model yields
  `{ kind: 'error', code, message }` — the host must show it, not paper over
  it.
- `LineResult.text === ''` means “below confidence”, not “blank field”.
- Laya anomalies carry `severity: 'info' | 'warn' | 'fault'`; the Studio maps
  these onto the shared amber/red review semantics.
- `Executor::available()` lets a host declare which `ModelKind`s it can serve;
  the engine skips optional stages (orient, script routing) instead of failing
  the page.

Next: [Rust crate API](sdk-rust-api.html).
