# web/ — NAYEN v1 public frontend

The immersive public page for the ICR Studios engine: cinematic 3D hero,
data-driven problem statement, an interactive inspector for the real pipeline
constants, and a live demo that runs the engine **in the browser** — or, while
the Rust/WASM engine is still being built, a clearly-labelled deterministic
simulation.

Vite + strict TypeScript + hand-written CSS. No UI framework, no Tailwind.
Design tokens are consumed from `../packages/theme/tokens.css` (single source
of truth — nothing is hardcoded that a token provides). The engine contract
types are imported from `../engine/src/types.ts`, never duplicated.

## Run locally

```bash
cd web
npm install
npm run dev        # http://localhost:5173
npm run build      # production build → dist/
npm run preview    # serve dist/
npm run typecheck  # tsc --noEmit
npm run lint       # eslint (flat config)
```

The dev server allows reads above `web/` (`server.fs.allow`) because the page
imports `packages/theme/tokens.css` and `engine/src/types.ts`.

## Architecture

```
web/
├─ index.html               semantic markup for every section; JS enhances, never gates
├─ vercel.json              .wasm/.onnx MIME + immutable asset caching + SPA rewrite
├─ vite.config.ts           base './', MIME fixes for dev/preview, ort chunk split
└─ src/
   ├─ main.ts               bootstrap: reveals → hero → pipeline → driver detect → telemetry/demo
   ├─ style.css             @imports the theme tokens; all layout/motion lives here
   ├─ hero.ts               pointer + scroll parallax on the CSS-3D stage (rAF, lerp)
   ├─ reveal.ts             scroll reveals: native animation-timeline: view(), IO fallback
   ├─ pipeline.ts           11 pipeline stages with params verbatim from nayen-core params.rs
   ├─ telemetry.ts          honest status strip / driver chip / footer provenance
   ├─ demo.ts               drop/paste/browse → decode → run → overlay + lines + timings + export
   └─ engine/
      ├─ driver.ts          NayenDriver interface + selectDriver() feature detection
      ├─ wasm-driver.ts     real inference: nayen_wasm glue + onnxruntime-web HostStep loop
      └─ simulated-driver.ts deterministic, unmistakably-labelled fallback
```

### The driver contract

```ts
interface NayenDriver {
  readonly name: string;        // telemetry identity
  readonly simulated: boolean;  // true ⇒ output is a labelled simulation
  run(
    image: ImageData,
    config: EngineConfig,
    onProgress: (e: ProgressEvent) => void,
  ): Promise<PageResult>;
}
```

`selectDriver()` HEAD-probes `wasm/nayen_wasm.js` and `models/*.onnx` at load.
If the glue **and** the core pair (`det.onnx`, `rec.onnx`) exist, the
`WasmDriver` is used; otherwise the `SimulatedDriver` takes over and every
surface says so (nav chip, hero status strip, viewer watermark, telemetry log,
export footers). Missing optional models degrade the pipeline; nothing is ever
faked.

`WasmDriver` pumps the request/response state machine from
`engine/src/types.ts`: `nayen_start` → `HostStep` → for each `InferRequest` it
runs `onnxruntime-web` (wasm-only distribution, `numThreads = 1`, no
cross-origin isolation needed) and hands outputs back through `nayen_feed` as
offsets/lengths into the WASM linear-memory scratch buffer. Tensor payloads
never cross as JSON.

### Swapping in the real engine

1. Build the glue: `wasm-pack build crates/nayen-wasm --target web` (or the
   repo release pipeline).
2. Copy `nayen_wasm.js` + `nayen_wasm_bg.wasm` into `web/public/wasm/`.
3. Drop ONNX weights into `web/public/models/`, named by `ModelKind`:
   `orient.onnx det.onnx script.onnx rec.onnx rec_world.onnx rec_cjk.onnx
   rec_rtl.onnx order.onnx` (provenance: `datalab-to/indra-ocr`, OpenRAIL —
   not redistributed here).
4. Reload. Feature detection flips the chip from `driver: simulated` to
   `driver: wasm` and the footer shows `nayen_version()`.

No code changes needed; the ort runtime binary ships as a bundler-emitted
hashed asset (only fetched when real inference is possible).

### Honesty rules baked into the UI

- Simulated line text is literally `[simulated line N]`; markdown exports open
  with a SIMULATED banner; JSON exports embed `driver.simulated`.
- Progress events the host cannot know (line counts, applied rotation) are
  rendered as bare stage names, never invented numbers.
- Colour encodes state only: teal = verified, amber = needs review, red =
  no text / fault. Every coloured state also carries a text label.

## Motion & accessibility

- Scroll reveals prefer native `animation-timeline: view()`; a single
  IntersectionObserver is the fallback. `prefers-reduced-motion` zeroes
  durations (tokens), flattens the 3D scene, and short-circuits reveals to
  their final state.
- The 3D hero is pure CSS transforms + a small rAF parallax; it flattens on
  coarse pointers, <960px viewports and low-power devices.
- Real heading hierarchy, skip link, visible focus rings, ≥44px touch
  targets, `aria-live` telemetry, alt text on loaded scans.

## Deployment

`vercel.json` sets `application/wasm` / `application/octet-stream` headers,
immutable caching for hashed `assets/`, and an SPA rewrite. `base: './'` keeps
the build hostable from any path. No COOP/COEP: the wasm backend is
single-threaded, so cross-origin isolation would break previews for nothing.
