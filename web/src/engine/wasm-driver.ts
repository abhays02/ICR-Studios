/**
 * WasmDriver — the real thing.
 *
 * Pumps the `HostStep` request/response loop documented in
 * `engine/src/types.ts`: the engine emits one `InferRequest` at a time; this
 * driver answers it with `onnxruntime-web` (single-threaded wasm backend, no
 * cross-origin isolation required) and hands outputs back via `nayen_feed`.
 * Tensor payloads travel as offsets/lengths into the WASM linear-memory
 * scratch buffer — never through JSON.
 *
 * Every dynamic import is guarded: a missing glue file, missing models or a
 * failing ort bootstrap surfaces as a descriptive error, and
 * `selectDriver()` degrades to the SimulatedDriver instead of throwing.
 *
 * Progress events: the host can only observe *which model ran and how many
 * times* — line counts, applied rotation and field counts are engine-internal
 * until `done`. Payloads the host cannot know are emitted as zero/unknown and
 * the UI renders them as bare stage names rather than inventing numbers.
 * When `engine/src/host.ts` lands it supersedes this mapping.
 */
import type {
  DType,
  EngineConfig,
  HostStep,
  InferRequest,
  ModelKind,
  NamedTensor,
  NayenInit,
  NayenWasm,
  PageResult,
  ProgressEvent,
} from '../../engine/src/types';
// Wasm-only distribution of onnxruntime-web: single-threaded wasm backend,
// no WebGPU/WebGL code, smaller emitted runtime binary.
import type * as OrtNs from 'onnxruntime-web/wasm';
import { assetUrl, type NayenDriver } from './driver';

type OrtModule = typeof OrtNs;
type OrtSession = OrtNs.InferenceSession;
type OrtTensor = OrtNs.Tensor;

interface GlueModule {
  default?: (input?: unknown) => Promise<unknown>;
  [exportName: string]: unknown;
}

const DTYPE_BYTES: Record<DType, number> = { f32: 4, i64: 8, bool: 1 };

function ortTypeToDType(t: OrtTensor): DType {
  switch (t.type) {
    case 'float32':
      return 'f32';
    case 'int64':
      return 'i64';
    case 'bool':
      return 'bool';
    default:
      throw new Error(`unsupported tensor dtype from ort: ${String(t.type)}`);
  }
}

/** Everything `selectDriver()` already proved is servable. */
export interface WasmDriverOptions {
  glueUrl: string;
  /** Models whose `.onnx` answered a HEAD request, in `MODEL_KINDS` order. */
  available: ModelKind[];
  /** Raw `models/meta.json` text — the CTC vocabularies. */
  meta: string;
  /** Opt into the deskew search. Off by default; see `NayenInit.deskew`. */
  deskew?: boolean;
}

export class WasmDriver implements NayenDriver {
  readonly name = 'wasm';
  readonly simulated = false;

  #opts: WasmDriverOptions;
  #ort: OrtModule | null = null;
  #wasm: NayenWasm | null = null;
  #memory: WebAssembly.Memory | null = null;
  #sessions = new Map<ModelKind, Promise<OrtSession>>();

  constructor(opts: WasmDriverOptions) {
    this.#opts = opts;
  }

  /** Load the engine and report `nayen_version()` — used by telemetry. */
  async probeVersion(): Promise<string> {
    await this.#init();
    return this.#wasmOrThrow().nayen_version();
  }

  async run(
    image: ImageData,
    config: EngineConfig,
    onProgress: (e: ProgressEvent) => void,
  ): Promise<PageResult> {
    await this.#init();
    const wasm = this.#wasmOrThrow();

    onProgress({ stage: 'decode-image' });

    // Copy RGBA pixels into the scratch buffer at offset 0.
    const rgbaBytes = image.width * image.height * 4;
    this.#ensureScratch(rgbaBytes);
    new Uint8Array(this.#buffer(), this.#scratchPtr(), rgbaBytes).set(
      new Uint8Array(image.data.buffer, image.data.byteOffset, rgbaBytes),
    );

    const init: NayenInit = {
      config,
      available: this.#opts.available,
      meta: this.#opts.meta,
      deskew: this.#opts.deskew ?? false,
    };
    const session = wasm.nayen_create(JSON.stringify(init));
    if (!Number.isFinite(session) || session <= 0) {
      // nayen_create reports the reason on console.error; it has no session id to
      // return a HostStep against yet.
      throw new Error(
        `nayen_create rejected the init payload (see console.error) — id was ${String(session)}`,
      );
    }

    const seen = { orient: 0, det: 0, script: 0, rec: 0, order: 0 };
    try {
      let step = this.#parseStep(wasm.nayen_start(session, 0, image.width, image.height));
      for (;;) {
        switch (step.kind) {
          case 'done':
            onProgress({ stage: 'done' });
            return step.result;
          case 'error':
            throw new Error(`engine error ${step.code}: ${step.message}`);
          case 'infer': {
            const outputs = await this.#infer(step.request);
            this.#countAndReport(step.request.model, seen, onProgress);
            step = this.#parseStep(wasm.nayen_feed(session, JSON.stringify(outputs)));
            break;
          }
        }
      }
    } finally {
      wasm.nayen_destroy(session);
    }
  }

  /* ------------------------------------------------------------- init */

  async #init(): Promise<void> {
    if (this.#wasm && this.#memory) return;

    let mod: GlueModule;
    try {
      mod = (await import(/* @vite-ignore */ this.#opts.glueUrl)) as GlueModule;
    } catch (err) {
      throw new Error(`failed to load wasm glue at ${this.#opts.glueUrl}: ${String(err)}`);
    }

    // The `--target web` glue does not re-export the linear memory as a module
    // binding, but its init function resolves to the raw instance exports, which
    // do carry it. That is the only supported way to reach the buffer.
    let exports: GlueModule | undefined;
    try {
      if (typeof mod.default === 'function') {
        exports = (await mod.default()) as GlueModule | undefined;
      }
    } catch (err) {
      throw new Error(`wasm glue instantiation failed: ${String(err)}`);
    }

    const required = [
      'nayen_create',
      'nayen_start',
      'nayen_feed',
      'nayen_destroy',
      'nayen_scratch_ptr',
      'nayen_scratch_len',
      'nayen_scratch_grow',
      'nayen_version',
    ] as const;
    for (const fn of required) {
      if (typeof mod[fn] !== 'function') {
        throw new Error(`wasm glue is missing export ${fn}() — contract drift with engine/src/types.ts`);
      }
    }

    let memory: WebAssembly.Memory | null = null;
    for (const source of [exports, mod]) {
      if (!source) continue;
      for (const value of Object.values(source)) {
        if (value instanceof WebAssembly.Memory) {
          memory = value;
          break;
        }
      }
      if (memory) break;
    }
    if (!memory) {
      throw new Error('wasm glue exports no WebAssembly.Memory — cannot exchange tensor payloads');
    }

    this.#memory = memory;
    this.#wasm = mod as unknown as NayenWasm;

    try {
      this.#ort ??= await import('onnxruntime-web/wasm');
      // Single-threaded wasm backend: no SharedArrayBuffer, no COOP/COEP.
      // The runtime binary ships as a bundler-emitted hashed asset.
      this.#ort.env.wasm.numThreads = 1;
      this.#ort.env.wasm.proxy = false;
    } catch (err) {
      throw new Error(`onnxruntime-web failed to load: ${String(err)}`);
    }
  }

  #wasmOrThrow(): NayenWasm {
    if (!this.#wasm) throw new Error('wasm engine not initialised');
    return this.#wasm;
  }

  /* --------------------------------------------------------- sessions */

  #session(kind: ModelKind): Promise<OrtSession> {
    let pending = this.#sessions.get(kind);
    if (!pending) {
      const ort = this.#ort;
      if (!ort) throw new Error('onnxruntime-web not initialised');
      const url = assetUrl(`models/${kind}.onnx`).href;
      pending = ort.InferenceSession.create(url, {
        executionProviders: ['wasm'],
        graphOptimizationLevel: 'all',
      });
      this.#sessions.set(kind, pending);
      // A failed load must not poison the cache permanently.
      pending.catch(() => this.#sessions.delete(kind));
    }
    return pending;
  }

  /* ---------------------------------------------------------- scratch */

  #buffer(): ArrayBuffer {
    const memory = this.#memory;
    if (!memory) throw new Error('wasm memory unavailable');
    return memory.buffer;
  }

  #scratchPtr(): number {
    return this.#wasmOrThrow().nayen_scratch_ptr();
  }

  #ensureScratch(bytes: number): void {
    const wasm = this.#wasmOrThrow();
    let guard = 0;
    while (wasm.nayen_scratch_len() < bytes) {
      const grown = wasm.nayen_scratch_grow(bytes);
      if (grown <= 0 || ++guard > 64) {
        throw new Error(`scratch buffer could not grow to ${bytes} bytes`);
      }
    }
  }

  /** Fresh typed view into scratch — must be re-created after any grow. */
  #scratchView(dtype: DType, offset: number, length: number):
    | Float32Array
    | BigInt64Array
    | Uint8Array {
    const ptr = this.#scratchPtr();
    switch (dtype) {
      case 'f32':
        return new Float32Array(this.#buffer(), ptr + offset, length);
      case 'i64':
        return new BigInt64Array(this.#buffer(), ptr + offset, length);
      case 'bool':
        return new Uint8Array(this.#buffer(), ptr + offset, length);
    }
  }

  /* ------------------------------------------------------------ infer */

  async #infer(request: InferRequest): Promise<NamedTensor[]> {
    const ort = this.#ort;
    if (!ort) throw new Error('onnxruntime-web not initialised');
    const session = await this.#session(request.model);

    const feeds: Record<string, OrtTensor> = {};
    for (const named of request.inputs) {
      const { dtype, shape, offset, length } = named.tensor;
      const data = this.#scratchView(dtype, offset, length);
      feeds[named.name] = new ort.Tensor(
        dtype === 'f32' ? 'float32' : dtype === 'i64' ? 'int64' : 'bool',
        // Copy out: the engine may grow scratch (detaching the buffer) while
        // ort still holds the view.
        dtype === 'f32'
          ? Float32Array.from(data as Float32Array)
          : dtype === 'i64'
            ? BigInt64Array.from(data as BigInt64Array)
            : Uint8Array.from(data as Uint8Array),
        shape.map((d) => (d < 0 ? 1 : d)),
      );
    }

    const results = await session.run(feeds);

    // Lay outputs out sequentially beyond the current scratch capacity and
    // report offsets back — placement is host-owned per the contract.
    const named: { name: string; tensor: OrtTensor; dtype: DType }[] = request.outputs.map(
      (name) => {
        const t = results[name];
        if (!t) throw new Error(`model ${request.model} produced no output "${name}"`);
        return { name, tensor: t, dtype: ortTypeToDType(t) };
      },
    );
    const totalBytes = named.reduce(
      (sum, o) => sum + o.tensor.size * DTYPE_BYTES[o.dtype],
      0,
    );
    let cursor = this.#wasmOrThrow().nayen_scratch_len();
    this.#ensureScratch(cursor + totalBytes);

    const outputs: NamedTensor[] = [];
    for (const o of named) {
      const length = o.tensor.size;
      const view = this.#scratchView(o.dtype, cursor, length);
      if (o.dtype === 'f32') (view as Float32Array).set(o.tensor.data as Float32Array);
      else if (o.dtype === 'i64') (view as BigInt64Array).set(o.tensor.data as BigInt64Array);
      else (view as Uint8Array).set(o.tensor.data as Uint8Array);
      outputs.push({
        name: o.name,
        tensor: {
          dtype: o.dtype,
          shape: [...o.tensor.dims],
          offset: cursor,
          length,
        },
      });
      cursor += length * DTYPE_BYTES[o.dtype];
    }
    return outputs;
  }

  /* --------------------------------------------------------- progress */

  #countAndReport(
    model: ModelKind,
    seen: { orient: number; det: number; script: number; rec: number; order: number },
    onProgress: (e: ProgressEvent) => void,
  ): void {
    switch (model) {
      case 'orient':
        seen.orient += 1;
        // degrees: engine-internal until `done` — 0 signals "unknown", the
        // UI renders a bare stage name for it.
        onProgress({ stage: 'orient', degrees: 0 });
        break;
      case 'det':
        seen.det += 1;
        onProgress({ stage: 'detect', lines: 0, pass: seen.det });
        break;
      case 'script':
        seen.script += 1;
        onProgress({ stage: 'route', groups: { base: 0, cjk: 0, rtl: 0, other: 0 } });
        break;
      case 'rec':
      case 'rec_world':
      case 'rec_cjk':
      case 'rec_rtl':
        seen.rec += 1;
        onProgress({ stage: 'recognize', batch: seen.rec, of: seen.rec });
        break;
      case 'order':
        seen.order += 1;
        onProgress({ stage: 'laya', fields: 0 });
        break;
    }
  }

  #parseStep(json: string): HostStep {
    try {
      return JSON.parse(json) as HostStep;
    } catch (err) {
      throw new Error(`engine returned malformed HostStep JSON: ${String(err)}`);
    }
  }
}
