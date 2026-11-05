# WASM integration

The browser host drives the exact same engine as the native CLI through a
small exported surface and a request/response loop. The engine emits
`InferRequest`s; the host awaits `onnxruntime-web` and feeds outputs back.
No threads, no atomics, no WASM-side event loop.

## The exported surface

%%include engine/src/types.ts#NayenWasm%%

## Driver loop

The JS half (implemented by `engine/src/host.ts`) is a loop, not a callback
web:

```ts
const session = wasm.nayen_create(JSON.stringify(config));
// Copy RGBA pixels into WASM linear memory at `offset`, then:
let step: HostStep = JSON.parse(wasm.nayen_start(session, offset, width, height));
while (step.kind === 'infer') {
  const outputs = await runOnnx(step.request);   // host-owned runtime
  writeScratch(wasm, outputs);                   // tensors stay in linear memory
  step = JSON.parse(wasm.nayen_feed(session, outputsJson));
}
wasm.nayen_destroy(session);
if (step.kind === 'error') showFailureEvidence(step);   // never fabricate text
else render(step.result);                               // PageResult
```

Rules the host must respect:

- **Tensors never travel through JSON.** `TensorRef.offset` / `length` index
  the scratch buffer obtained via `nayen_scratch_ptr()`; grow it with
  `nayen_scratch_grow(bytes)` before writing large outputs.
- **One outstanding request.** Feed the outputs of the previous
  `InferRequest` before asking for the next step.
- **`outputs` order matters.** Return the named outputs in the order the
  engine requested them.
- **Errors are results.** `{ kind: 'error', code, message }` must be surfaced
  as failure evidence in the UI; the loop ends there.

## Memory and page bounds

Use the browser preset so a 600 dpi A3 scan cannot exhaust the WASM heap:

```ts
const config: EngineConfig = { ...DEFAULT_CONFIG, profile: 'handwriting' };
// EngineConfig.browser() on the Rust side: max_page_side 1800, skew_side 480.
```

Optional stages degrade, they never fabricate: a host that loaded only
`det.onnx` + `rec.onnx` still runs — orientation and script routing are
skipped based on `Executor::available()`, and `PageResult.modelsUsed` reports
what actually ran.

## Weight provisioning

The WASM bundle ships **without models**. Weight files (`orient`, `det`,
`script`, `rec`, `rec_world`, `rec_cjk`, `rec_rtl`, `order`) must be
provisioned explicitly by the host, with provenance recorded — see
[model provenance and licensing](sdk-provenance.html). A missing weight file
produces `HostStep.error`, which every NAYEN surface renders as an honest
“engine not loaded / model unavailable” state.

## Progress

Emit `ProgressEvent`s from the driver loop as stages change (`decode-image`,
`orient`, `detect`, `route`, `recognize`, `laya`, `done`) so UIs can show real
transitions instead of synthetic percentages. Per-stage wall-clock
milliseconds arrive at the end in `PageResult.timings`.
