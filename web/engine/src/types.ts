/**
 * NAYEN v1 engine host contract.
 *
 * This file IS the contract between the Rust core (`crates/nayen-core`) and
 * every host that drives it. It is hand-maintained and must stay in sync with
 * `crates/nayen-wasm/src/lib.rs`; `npm run check:contract` diffs the two.
 *
 * ---------------------------------------------------------------------------
 * Why a request/response state machine
 * ---------------------------------------------------------------------------
 * `onnxruntime-web`'s `session.run()` returns a Promise, and Rust compiled to
 * WASM cannot block on a JS promise. So the engine never calls the runtime
 * itself. It emits one `InferRequest` at a time and waits to be handed the
 * outputs back. That makes the same engine code work:
 *
 *   - native  : `nayen-cli` loops synchronously over `ort`
 *   - browser : this driver awaits `onnxruntime-web` between steps
 *
 * Neither host needs threads, atomics, or a WASM-side event loop.
 */

/** Detection/crop preset. `indra` reproduces the reference pipeline exactly. */
export type Profile = 'indra' | 'handwriting' | 'sweep';

/** Weight files the engine may ask for. */
export type ModelKind =
  | 'orient'
  | 'det'
  | 'script'
  | 'rec'
  | 'rec_world'
  | 'rec_cjk'
  | 'rec_rtl'
  | 'order';

export type DType = 'f32' | 'i64' | 'bool';

/** A dense tensor crossing the WASM boundary. `shape` may contain -1 for a
 *  dynamic axis on input; the resolved shape comes back on output. */
export interface TensorRef {
  dtype: DType;
  shape: number[];
  /** Index into the WASM linear-memory scratch buffer the host reads/writes.
   *  Raw bytes are never copied through JSON — see `engine/src/host.ts`. */
  offset: number;
  length: number;
}

export interface NamedTensor {
  name: string;
  tensor: TensorRef;
}

/** One inference call the host must perform. */
export interface InferRequest {
  model: ModelKind;
  inputs: NamedTensor[];
  /** Output names, in the order the engine wants them returned. */
  outputs: string[];
}

/** Full engine configuration a host sends in. Mirrors the `JsEngineConfig`
 *  half of `crates/nayen-wasm/src/lib.rs` — detection and crop geometry are not
 *  settable here, they come from the `profile`. */
export interface EngineConfig {
  profile: Profile;
  /** Lines below this mean CTC confidence yield no text (box is kept). */
  minConf: number;
  /** Run `orient.onnx` and apply quarter turns. */
  orient: boolean;
  /** Run `script.onnx` to route non-Latin lines. */
  routeScripts: boolean;
  /** Run Laya field extraction and cross-checks after recognition. */
  laya: boolean;
  /** Cap on the page's longest side, to bound WASM heap use. */
  maxPageSide: number;
}

/** What the host hands `nayen_create`. */
export interface NayenInit {
  config: EngineConfig;
  /** Models the host can actually serve. Anything absent is skipped with a note
   *  in `PageResult.notes` — the engine never fabricates output for a model it
   *  does not have. */
  available: ModelKind[];
  /** Raw `meta.json` text. Required: it carries the recognizer vocabularies, so
   *  without it no text can be decoded at all. */
  meta: string;
  /** Opt into the deskew search. Defaults to false. */
  deskew?: boolean;
}

/** A detected text line, in original page pixels. */
export interface LineBox {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  /** Mean sigmoid probability over the line's core band. */
  score: number;
}

/** Which recognizer produced a line. `base` is `rec.onnx` (Latin). */
export type ScriptGroup = 'base' | 'cjk' | 'rtl' | 'other';

export interface LineResult {
  box: LineBox;
  /** Empty string when confidence was below `minConf` — never a guess. */
  text: string;
  /** Mean CTC peak probability over the kept tokens. */
  conf: number;
  script: ScriptGroup;
  /** Set when Laya decided this line needs human eyes. */
  needsReview: boolean;
  /** Per-character page-x positions, for overlay highlighting. */
  charX: number[];
  /** Why the line is flagged, so a reviewer sees a reason and not just a colour. */
  note: string | null;
}

/** Laya's structured read of a banking form. */
export interface ExtractedField {
  name: string;
  value: string;
  conf: number;
  /** Null when the field was not found. Absence is reported, not filled in. */
  valid: boolean | null;
  rule: string;
  /** Index into `PageResult.lines` this value was read from. */
  line: number;
}

export interface Anomaly {
  code: string;
  message: string;
  severity: 'info' | 'warn' | 'fault';
  lineIndex: number | null;
}

export interface FieldExtraction {
  docType: string | null;
  fields: ExtractedField[];
  anomalies: Anomaly[];
  /** Cross-check of amount-in-words against amount-in-figures. */
  carLarMatch: boolean | null;
}

/** Per-stage wall-clock milliseconds, measured inside the engine. */
export interface StageTimings {
  orient: number;
  deskew: number;
  detect: number;
  crop: number;
  route: number;
  recognize: number;
  laya: number;
  total: number;
}

export interface PageResult {
  width: number;
  height: number;
  lines: LineResult[];
  /** Reading-ordered Markdown. Blank lines still appear as layout evidence. */
  markdown: string;
  fields: FieldExtraction | null;
  timings: StageTimings;
  /** The profile that produced this page. Everything the profile resolves —
   *  detector thresholds, crop geometry — is fixed by this name, so the result is
   *  reproducible from the JSON alone. */
  profile: Profile;
  /** Models the engine actually used, so the UI can show real provenance. */
  modelsUsed: ModelKind[];
  /** Degrees counter-clockwise the page was turned to become upright. */
  rotation: number;
  /** Quarter turns applied from `orient.onnx`. */
  quarterTurns: number;
  /** Non-fatal degradations: a stage skipped because a model was absent, an
   *  adaptive pass that did not help. Surfaced, never silent. */
  notes: string[];
}

/** What the engine asks of the host next. */
export type HostStep =
  | { kind: 'infer'; request: InferRequest }
  | { kind: 'done'; result: PageResult }
  | { kind: 'error'; code: string; message: string };

/**
 * The WASM module's exported surface. `engine/src/host.ts` implements the JS
 * half; `crates/nayen-wasm` implements the Rust half.
 */
export interface NayenWasm {
  /** Create a session from a `NayenInit` payload. Returns an opaque session id,
   *  or 0 with the reason on `console.error`. Does not touch the scratch buffer:
   *  the host writes the page RGBA there first. */
  nayen_create(initJson: string): number;
  /** Begin a page. `rgba` is copied into WASM memory by the caller.
   *  Returns a `HostStep` as JSON (tensor payloads stay in linear memory). */
  nayen_start(session: number, rgbaOffset: number, width: number, height: number): string;
  /** Hand back the outputs of the previous `InferRequest`. */
  nayen_feed(session: number, outputsJson: string): string;
  /** Release a session and its scratch buffers. */
  nayen_destroy(session: number): void;
  /** Read/write the shared scratch buffer for tensor payloads. */
  nayen_scratch_ptr(): number;
  nayen_scratch_len(): number;
  nayen_scratch_grow(bytes: number): number;
  /** Engine version and build metadata, for the UI's provenance strip. */
  nayen_version(): string;
}

/** Progress events surfaced while the driver loop runs. */
export type ProgressEvent =
  | { stage: 'decode-image' }
  | { stage: 'orient'; degrees: number }
  | { stage: 'detect'; lines: number; pass: number }
  | { stage: 'route'; groups: Record<ScriptGroup, number> }
  | { stage: 'recognize'; batch: number; of: number }
  | { stage: 'laya'; fields: number }
  | { stage: 'done' };

export const DEFAULT_CONFIG: EngineConfig = {
  profile: 'indra',
  minConf: 0.5,
  orient: true,
  routeScripts: true,
  laya: true,
  maxPageSide: 1800,
};
