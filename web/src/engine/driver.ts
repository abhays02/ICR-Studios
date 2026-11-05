/**
 * NAYEN v1 web driver seam.
 *
 * One interface, two implementations:
 *   - WasmDriver      — real inference: nayen_wasm glue + onnxruntime-web.
 *   - SimulatedDriver — honest, deterministic, clearly-labelled fallback so
 *                       the UI is testable before the Rust engine ships.
 *
 * `selectDriver()` feature-detects the wasm glue and the ONNX models over
 * HTTP and never throws: a missing file degrades to the simulation.
 */
import type { EngineConfig, ModelKind, PageResult, ProgressEvent } from '../../engine/src/types';

export interface NayenDriver {
  /** Human-readable driver identity, shown in telemetry. */
  readonly name: string;
  /** True when output is a labelled simulation, never real OCR. */
  readonly simulated: boolean;
  run(
    image: ImageData,
    config: EngineConfig,
    onProgress: (e: ProgressEvent) => void,
  ): Promise<PageResult>;
}

/** Everything the UI needs to be honest about what will run. */
export interface DriverInfo {
  driver: NayenDriver;
  /** Short label for chips, e.g. "simulated" or "wasm". */
  label: string;
  /** One-line explanation for the status strip. */
  detail: string;
  wasmGluePresent: boolean;
  /** Raw `meta.json` text when it was served, else null. Carries the recognizer
   *  vocabularies, so real inference is impossible without it. */
  metaJson: string | null;
  modelsPresent: ModelKind[];
  modelsMissing: ModelKind[];
  /** From `nayen_version()` when the wasm engine loaded, else null. */
  engineVersion: string | null;
}

export const MODEL_KINDS: ModelKind[] = [
  'orient',
  'det',
  'script',
  'rec',
  'rec_world',
  'rec_cjk',
  'rec_rtl',
  'order',
];

/** Resolve a public/ asset URL against the deployed base path. */
export function assetUrl(relative: string): URL {
  // `import.meta.env.BASE_URL` is './' (vite base) — resolve against the
  // document so the page works from any host path.
  return new URL(relative, document.baseURI);
}

async function exists(url: URL): Promise<boolean> {
  try {
    const res = await fetch(url, { method: 'HEAD' });
    if (!res.ok) return false;
    // SPA rewrites answer 200 with index.html for missing files — reject that.
    const type = res.headers.get('content-type') ?? '';
    return !type.includes('text/html');
  } catch {
    return false;
  }
}

export async function selectDriver(): Promise<DriverInfo> {
  const glueUrl = assetUrl('wasm/nayen_wasm.js');
  const metaUrl = assetUrl('models/meta.json');
  const [wasmGluePresent, metaJson] = await Promise.all([
    exists(glueUrl),
    fetchText(metaUrl),
  ]);

  const present: ModelKind[] = [];
  const missing: ModelKind[] = [];
  await Promise.all(
    MODEL_KINDS.map(async (kind) => {
      if (await exists(assetUrl(`models/${kind}.onnx`))) present.push(kind);
      else missing.push(kind);
    }),
  );
  present.sort((a, b) => MODEL_KINDS.indexOf(a) - MODEL_KINDS.indexOf(b));
  missing.sort((a, b) => MODEL_KINDS.indexOf(a) - MODEL_KINDS.indexOf(b));

  // The engine degrades on missing optional models, but det + rec are the
  // minimum for any real run, and meta.json is not optional at all: it holds the
  // CTC vocabularies, so without it the recognizer cannot decode a single token.
  const coreReady =
    wasmGluePresent && metaJson !== null && present.includes('det') && present.includes('rec');

  if (coreReady) {
    // Lazy import so the ort chunk is only fetched when real inference is
    // actually possible.
    const { WasmDriver } = await import('./wasm-driver');
    const driver = new WasmDriver({ glueUrl: glueUrl.href, available: present, meta: metaJson });
    const engineVersion = await driver.probeVersion().catch(() => null);
    return {
      driver,
      label: 'wasm',
      detail: `live inference · ${present.length}/${MODEL_KINDS.length} models`,
      wasmGluePresent,
      metaJson,
      modelsPresent: present,
      modelsMissing: missing,
      engineVersion,
    };
  }

  const { SimulatedDriver } = await import('./simulated-driver');
  const reasons: string[] = [];
  if (!wasmGluePresent) reasons.push('wasm glue absent');
  if (metaJson === null) reasons.push('meta.json absent');
  if (missing.includes('det') || missing.includes('rec')) reasons.push('core models absent');
  return {
    driver: new SimulatedDriver(),
    label: 'simulated',
    detail: reasons.join(' · ') || 'engine unavailable',
    wasmGluePresent,
    metaJson,
    modelsPresent: present,
    modelsMissing: missing,
    engineVersion: null,
  };
}

/** Fetch a text asset, or null when it is absent or served as an SPA fallback. */
async function fetchText(url: URL): Promise<string | null> {
  try {
    const res = await fetch(url);
    if (!res.ok) return null;
    const type = res.headers.get('content-type') ?? '';
    if (type.includes('text/html')) return null;
    return await res.text();
  } catch {
    return null;
  }
}
