/**
 * SimulatedDriver — the honest fallback.
 *
 * Produces a DETERMINISTIC visual walkthrough of the pipeline so the UI is
 * fully testable before the Rust/WASM engine exists. It is labelled as a
 * simulation at every level:
 *   - line text is literally `[simulated line N]`, never plausible OCR;
 *   - results carry a SIMULATED_DRIVER anomaly and an explanatory markdown
 *     banner;
 *   - no models are claimed (`modelsUsed: []`);
 *   - timings are real wall-clock milliseconds of the simulation itself.
 *
 * Determinism: boxes/confidences derive from a seeded PRNG keyed on the
 * image dimensions + profile, so the same input always gives the same
 * walkthrough.
 */
import type {
  EngineConfig,
  FieldExtraction,
  LineResult,
  PageResult,
  ProgressEvent,
  StageTimings,
} from '../../engine/src/types';
import type { NayenDriver } from './driver';

/** Small deterministic PRNG (mulberry32). */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const prefersReducedMotion = (): boolean =>
  typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

export class SimulatedDriver implements NayenDriver {
  readonly name = 'simulated';
  readonly simulated = true;

  async run(
    image: ImageData,
    config: EngineConfig,
    onProgress: (e: ProgressEvent) => void,
  ): Promise<PageResult> {
    const reduced = prefersReducedMotion();
    const wait = (ms: number): Promise<void> =>
      new Promise((resolve) => setTimeout(resolve, reduced ? 0 : ms));

    const seed =
      (image.width * 73856093) ^ (image.height * 19349663) ^ (config.profile.length * 83492791);
    const rng = mulberry32(seed);

    const timings: StageTimings = {
      orient: 0,
      deskew: 0,
      detect: 0,
      crop: 0,
      route: 0,
      recognize: 0,
      laya: 0,
      total: 0,
    };
    const t0 = performance.now();
    let mark = t0;
    const lap = (key: keyof Omit<StageTimings, 'total'>): void => {
      const now = performance.now();
      timings[key] = now - mark;
      mark = now;
    };

    onProgress({ stage: 'decode-image' });
    await wait(160);

    if (config.orient) {
      await wait(220);
      // The simulation never invents a rotation: pages are reported upright.
      onProgress({ stage: 'orient', degrees: 0 });
      lap('orient');
    }

    // Deterministic line layout inside the page margins.
    const n = 8 + Math.floor(rng() * 6); // 8..13 lines
    const marginX = Math.max(8, Math.round(image.width * 0.08));
    const top = Math.max(8, Math.round(image.height * 0.1));
    const usableH = Math.max(image.height - top * 2, n * 12);
    const step = usableH / n;

    const lines: LineResult[] = [];
    for (let i = 0; i < n; i++) {
      const h = Math.max(6, Math.round(step * 0.44 * (0.8 + rng() * 0.5)));
      const y0 = Math.round(top + i * step);
      const x0 = marginX + Math.round(rng() * marginX * 0.4);
      const w = Math.round((0.42 + rng() * 0.5) * (image.width - 2 * marginX));
      // Deterministic state mix so the demo always shows all three
      // confidence states: ~1/5 no-text (below MIN_CONF), ~1/3 needs-review,
      // the rest verified.
      const conf =
        i % 5 === 4 ? 0.16 + rng() * 0.26 : i % 3 === 1 ? 0.6 + rng() * 0.18 : 0.82 + rng() * 0.16;
      const below = conf < config.minConf;
      const text = below ? '' : `[simulated line ${i + 1}]`;
      const charX: number[] = [];
      if (text) {
        const per = Math.max(w / text.length, 1);
        for (let c = 0; c < text.length; c++) charX.push(Math.round(x0 + c * per));
      }
      lines.push({
        box: {
          x0,
          y0,
          x1: Math.min(x0 + w, image.width - 1),
          y1: Math.min(y0 + h, image.height - 1),
          score: Math.round((0.55 + rng() * 0.44) * 1000) / 1000,
        },
        text,
        conf: Math.round(conf * 1000) / 1000,
        script: 'base',
        needsReview: below || conf < 0.8,
        charX,
        note: null,
      });
    }

    await wait(280);
    onProgress({ stage: 'detect', lines: lines.length, pass: 1 });
    lap('detect');

    await wait(140);
    lap('crop');

    if (config.routeScripts) {
      await wait(180);
      onProgress({ stage: 'route', groups: { base: lines.length, cjk: 0, rtl: 0, other: 0 } });
    }

    await wait(320);
    onProgress({ stage: 'recognize', batch: 1, of: 1 });
    lap('recognize');

    let fields: FieldExtraction | null = null;
    if (config.laya) {
      await wait(200);
      const kept = lines.filter((l) => l.text !== '');
      fields = {
        docType: null,
        fields: [
          {
            name: 'amount_words',
            value: '[simulated]',
            conf: kept.length ? kept[0]!.conf : 0,
            valid: null,
            rule: 'simulated — no real validation performed',
            line: 0,
          },
          {
            name: 'amount_figures',
            value: '[simulated]',
            conf: kept.length > 1 ? kept[1]!.conf : 0,
            valid: null,
            rule: 'simulated — no real validation performed',
            line: 1,
          },
        ],
        anomalies: [
          {
            code: 'SIMULATED_DRIVER',
            message:
              'Simulated driver active — the NAYEN WASM engine is not loaded. Nothing here is OCR output.',
            severity: 'warn',
            lineIndex: null,
          },
        ],
        carLarMatch: null,
      };
      onProgress({ stage: 'laya', fields: fields.fields.length });
      lap('laya');
    }

    const markdown = [
      '> **SIMULATED transcript — the NAYEN WASM engine is not loaded.**',
      '> No OCR was performed. Text below is placeholder labelling, not recognition output.',
      '',
      ...lines.map((l, i) =>
        l.text
          ? `${String(i + 1).padStart(2, '0')} · ${l.text} · conf ${l.conf.toFixed(2)}${l.needsReview ? ' · REVIEW' : ''}`
          : `${String(i + 1).padStart(2, '0')} · (no text — below MIN_CONF ${config.minConf}) · box kept as layout evidence`,
      ),
      '',
    ].join('\n');

    timings.total = performance.now() - t0;
    onProgress({ stage: 'done' });

    return {
      width: image.width,
      height: image.height,
      lines,
      markdown,
      fields,
      timings,
      profile: config.profile,
      modelsUsed: [],
      rotation: 0,
      quarterTurns: 0,
      notes: [
        'simulated driver: no model ran, so no orientation, deskew or reading order was computed',
      ],
    };
  }
}
