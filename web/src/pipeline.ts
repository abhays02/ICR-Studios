/**
 * Pipeline inspector — the "prove the use case" centrepiece.
 *
 * Every constant below is lifted verbatim from
 * `crates/nayen-core/src/params.rs` (and the browser preset from
 * `EngineConfig::browser`). Where the three profiles disagree, all three
 * values are shown in order: indra / handwriting / sweep.
 */

interface Param {
  k: string;
  v: string;
}

interface StageDef {
  name: string;
  model: string | null;
  desc: string;
  params: Param[];
  note?: string;
}

export const STAGES: StageDef[] = [
  {
    name: 'page',
    model: null,
    desc: 'Decode locally (the image never leaves the machine), convert to grey via luma, and bound the page so a 600dpi scan cannot exhaust the WASM heap. Geometry is padded to a multiple of 32 — structural, identical in every profile.',
    params: [
      { k: 'max_page_side', v: '1800 browser · 2600 native' },
      { k: 'INPUT_MULTIPLE', v: '32' },
      { k: 'ink map', v: '1.0 = black ink, inverted' },
    ],
  },
  {
    name: 'orient',
    model: 'orient.onnx',
    desc: 'Quarter-turn correction. A turn is applied only at ≥ 0.95 confidence — real pages are almost always upright and one wrong rotation loses the whole page. Skipped entirely when the host did not load the model.',
    params: [
      { k: 'ORIENT_SIDE', v: '768' },
      { k: 'ORIENT_MIN', v: '0.95' },
      { k: 'ORIENT_VERTICAL_CLASS', v: '4' },
    ],
  },
  {
    name: 'deskew',
    model: null,
    desc: 'Slant search over the grey page before detection, using whole-page median ink statistics. No rotation is invented: if the search does not beat its own threshold the page stays as it is.',
    params: [
      { k: 'input', v: 'Gray8 · page median' },
      { k: 'search constants', v: 'not profiled in params.rs' },
    ],
    note: 'Deskew tunables live in the detector preprocessing path in crates/nayen-core, not in the profile system — profiles change post-processing and crop geometry only, never recognizer weights.',
  },
  {
    name: 'LineNet detect',
    model: 'det.onnx',
    desc: 'Text-line detection at reduced resolution. The profile threshold decides whether faint pencil and ballpoint strokes connect into components at all — this is where the reference pipeline loses handwriting. If the median line height comes in under ADAPTIVE_MIN_H, one re-detection pass runs at a larger scale; it only wins if it finds more lines.',
    params: [
      { k: 'thresh', v: '0.5 / 0.3 / 0.35' },
      { k: 'min_area', v: '2 / 1 / 1' },
      { k: 'long_side', v: '1024' },
      { k: 'adaptive', v: 'true / true / false' },
      { k: 'ADAPTIVE_MIN_H', v: '9.0' },
      { k: 'ADAPTIVE_TARGET_H', v: '12.0' },
      { k: 'ADAPTIVE_MAX_SIDE', v: '2048' },
      { k: 'ADAPTIVE_MIN_BOXES', v: '8' },
    ],
  },
  {
    name: 'decode boxes',
    model: null,
    desc: 'Turn the network output map into line boxes: connected components over the core channel, sized against the core band. The first four constants are structural — shared with training, changing them desynchronises decode. grow_* are safe post-processing: they absorb the ±1px error of the 1/2-resolution map, and for handwriting the ascenders and descenders that sit outside the core band.',
    params: [
      { k: 'CORE_FRAC', v: '0.4' },
      { k: 'CORE_X_SHRINK', v: '0.15' },
      { k: 'OUT_STRIDE', v: '2' },
      { k: 'grow_y', v: '0.0 / 0.12 / 0.1' },
      { k: 'grow_x', v: '0.0 / 0.08 / 0.06' },
    ],
  },
  {
    name: 'crop',
    model: null,
    desc: 'Each box becomes the crop the recognizer was trained on: 32px line height, windowed to 96px for rec.onnx. Widths pad to the recognizer timestep stride. The handwriting profile widens the margins and adds a contrast stretch so faint pencil reaches the dynamic range the recognizer knows.',
    params: [
      { k: 'LINE_HEIGHT', v: '32' },
      { k: 'window', v: '3 (rec) · 1 (script)' },
      { k: 'effective height', v: '96 = 32 × 3' },
      { k: 'pad_y', v: '0.2 / 0.35 / 0.2' },
      { k: 'pad_x', v: '0.3 / 0.4 / 0.3' },
      { k: 'WIDTH_MULTIPLE', v: '8' },
      { k: 'MAX_CROP_WIDTH', v: '4096' },
      { k: 'normalize_contrast', v: 'false / true / false' },
    ],
    note: "Effective vertical pad after the window transform: pad_y' = (window · (1 + 2·pad_y) − 1) / 2.",
  },
  {
    name: 'script route',
    model: 'script.onnx',
    desc: 'The script classifier reads the plain 32px crop and routes non-Latin lines to the world / CJK / RTL recognizers — but only when it beats ROUTE_SURE. Below that the base recognizer also reads the line and the more confident result wins. Routing is an optimisation, never a gate.',
    params: [
      { k: 'ROUTE_SURE', v: '0.95' },
      { k: 'script crop', v: 'window 1 → 32px' },
      { k: 'groups', v: 'base · cjk · rtl · other' },
    ],
  },
  {
    name: 'CTC recognize',
    model: 'rec.onnx (+ rec_world / rec_cjk / rec_rtl)',
    desc: 'CTC recognition over width-batched 96px crops — batches group lines of similar width so padding stays cheap. Below MIN_CONF mean peak probability a line yields no text; its box is kept, because layout and reading order still need it. Never a guess.',
    params: [
      { k: 'MIN_CONF', v: '0.5' },
      { k: 'MAX_BATCH', v: '64' },
      { k: 'MAX_WIDTH_RATIO', v: '1.3' },
      { k: 'MIN_BATCH_WIDTH', v: '64' },
      { k: 'MAX_BATCH_AREA', v: '64 × 640' },
    ],
  },
  {
    name: 'collapse / dedupe',
    model: null,
    desc: 'CTC collapses repeated tokens and blanks. Gaps between character centres — measured in line heights — then split one detected line into separate fields or comb-box cells. The sweep profile merges boxes from its multi-scale passes here and dedupes overlaps.',
    params: [
      { k: 'SPLIT_GAP', v: '3.0' },
      { k: 'CELL_GAP', v: '1.8' },
      { k: 'GUTTER_GAP', v: '1.6' },
    ],
  },
  {
    name: 'Laya decide',
    model: null,
    desc: 'The decision layer: field extraction, IFSC/PAN shape rules, amount-in-words (CAR) against amount-in-figures (LAR) cross-checks, confidence gating and needs-review flags. Laya annotates — it never rewrites evidence. Absence is reported, not filled in.',
    params: [
      { k: 'gate', v: 'MIN_CONF 0.5 upstream' },
      { k: 'cross-check', v: 'carLarMatch' },
      { k: 'output', v: 'needsReview per line' },
    ],
    note: 'Policy, not weights: Laya runs the same in nayen-cli, the Studio and this page.',
  },
  {
    name: 'fields + markdown',
    model: null,
    desc: 'The PageResult: reading-ordered lines with per-character x positions, markdown that keeps blank lines as layout evidence, validated fields with anomalies, per-stage wall-clock timings and the list of models actually used — real provenance, so the UI never claims a model that did not run.',
    params: [
      { k: 'lines[]', v: 'box · text · conf · script · needsReview · charX' },
      { k: 'markdown', v: 'reading order, blanks kept' },
      { k: 'timings', v: 'orient · detect · crop · recognize · laya · total' },
      { k: 'modelsUsed', v: 'provenance for the UI' },
    ],
  },
];

const DEFAULT_STAGE = 3; // LineNet detect — the centrepiece

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

function renderStage(index: number): void {
  const panel = document.getElementById('stage-detail');
  const stage = STAGES[index];
  if (!panel || !stage) return;
  panel.replaceChildren();

  const head = el('div', 'stage-detail__head');
  head.append(el('span', 'stage-detail__i', String(index).padStart(2, '0')));
  head.append(Object.assign(el('h3', '', stage.name)));
  if (stage.model) {
    const chip = el('span', 'chip chip--ok chip--sm stage-detail__model', stage.model);
    head.append(chip);
  } else {
    head.append(el('span', 'chip chip--neutral chip--sm stage-detail__model', 'no weights'));
  }
  panel.append(head);

  panel.append(el('p', 'stage-detail__desc', stage.desc));

  const params = el('dl', 'params');
  for (const p of stage.params) {
    const cell = el('div', 'params__cell');
    cell.append(el('dt', '', p.k));
    cell.append(el('dd', '', p.v));
    params.append(cell);
  }
  if (stage.note) params.append(el('p', 'params__note', stage.note));
  panel.append(params);
}

export function initPipeline(): void {
  const rail = document.getElementById('stage-rail');
  if (!rail) return;
  const buttons = [...rail.querySelectorAll<HTMLButtonElement>('.stage')];

  const select = (index: number, focus = false): void => {
    buttons.forEach((btn, i) => {
      btn.setAttribute('aria-current', i === index ? 'true' : 'false');
      if (i === index && focus) btn.focus();
    });
    renderStage(index);
  };

  buttons.forEach((btn, i) => {
    btn.addEventListener('click', () => select(i));
    btn.addEventListener('keydown', (event) => {
      const key = event.key;
      if (key === 'ArrowDown' || key === 'ArrowRight') {
        event.preventDefault();
        select((i + 1) % buttons.length, true);
      } else if (key === 'ArrowUp' || key === 'ArrowLeft') {
        event.preventDefault();
        select((i - 1 + buttons.length) % buttons.length, true);
      }
    });
  });

  select(DEFAULT_STAGE);
}
