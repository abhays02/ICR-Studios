# NAYEN v1 architecture

NAYEN v1 is the ICR Studios inference engine: one Rust implementation of
document perception and decision that runs natively (`ort`) and in the browser
(WASM) without forking its logic. It replaces the previous single-adapter
(TrOCR-only) pipeline with a pluggable **executor seam**.

> **Status, honestly.** `crates/nayen-core` carries the engine logic;
> `crates/nayen-wasm` and `crates/nayen-cli` are scaffolded and being filled
> in. The Studio build you can download today ships the engine **not loaded**:
> OCR jobs report failure evidence instead of predictions, and nothing invents
> text. See [model provenance and licensing](sdk-provenance.html) before
> provisioning any weights.

## Two layers: Indra and Laya

**Indra — perception.** Finds and reads ink:

1. `orient` — quarter-turn orientation classification (applied only above 0.95
   confidence; a wrong rotation loses the whole page).
2. `detect` — LineNet-style text-line detection; the network emits a core
   channel plus top/bottom distance maps at half resolution.
3. `crop` — line cropping to the 32px recognizer geometry, with skew search on
   a bounded thumbnail.
4. `recognize` — CTC recognition, batched by width; `script` routing sends
   non-Latin lines to `rec_cjk` / `rec_rtl` when the classifier is sure
   (≥ 0.95), otherwise the base recognizer also reads the line and the more
   confident result wins.

**Laya — decision.** Interprets readings against banking-form rules: field
extraction, CAR/LAR amount cross-checks, anomalies with severities
(`info` / `warn` / `fault`), and the `needsReview` flag that routes a line to a
human. Every Laya rule is deterministic and auditable; a field it cannot read
or validate is reported absent or invalid **with the reason** — never filled
with a guess.

## The executor seam

`nayen-core` never links an inference runtime. All model execution goes
through one trait:

```rust
pub trait Executor {
    fn run(&mut self, req: &InferRequest) -> Result<Vec<Tensor>, EngineError>;
    /// Which models this backend can actually serve. Lets the engine skip the
    /// orientation or script stages on a host that only loaded the core pair.
    fn available(&self) -> &[ModelKind];
}
```

Consequences:

- **Native hosts** (`nayen-cli`, the Studio backend) implement `Executor` over
  `ort` and loop synchronously.
- **Browser hosts** implement it over `onnxruntime-web`. Because
  `session.run()` is a Promise and WASM cannot block on one, the engine is a
  **request/response state machine**: it emits one `InferRequest`, the host
  awaits the runtime, and feeds outputs back. Details and the full WASM export
  surface: [host protocol](sdk-host-protocol.html) and
  [WASM integration](sdk-wasm.html).
- The same perception/decision code compiles unchanged to
  `wasm32-unknown-unknown`. `unsafe_code` is forbidden crate-wide.

## Where the Studio fits

The Studio operator workspace consumes engine output as *evidence*: detected
regions, recognized raw text, per-stage timings and models-used provenance.
Human corrections are stored separately from model evidence, and approval is
always explicit. Confidence semantics are one mapping across every NAYEN
surface: **signal teal = verified / high confidence**, **amber = needs human
review**, **fault red = invalid / rejected**.

## Reading order

1. [Host protocol and the executor seam](sdk-host-protocol.html)
2. [Rust crate API](sdk-rust-api.html)
3. [WASM integration](sdk-wasm.html)
4. [Profiles and parameters](sdk-profiles.html)
5. [Model provenance and licensing](sdk-provenance.html)
