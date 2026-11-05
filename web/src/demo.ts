/**
 * Live demo — drop/paste/browse an image, run the active driver, render
 * boxes + per-line results + timings + exports. Everything is local: the
 * file is decoded via canvas in this tab and never leaves the page.
 */
import type {
  EngineConfig,
  LineResult,
  PageResult,
  Profile,
  ProgressEvent,
} from '../../engine/src/types';
import type { DriverInfo } from './engine/driver';
import { assetUrl } from './engine/driver';

/**
 * Shipped forms the demo can load without the visitor having a scan to hand.
 * These are the synthetic CC0 renders from `data/samples`, chosen to exercise
 * different parts of Laya rather than to look impressive:
 *
 *   05-cheque            CAR/LAR cross-check agreeing at 7025
 *   04-rtgs              the same check across a paise boundary (12,500.50)
 *   06-car-skew-invalid  a tilted page with an impossible date, which Laya faults
 *
 * They are printed in a script font, not handwriting, and the note in the UI says
 * so: a synthetic render proves the pipeline runs, not that it reads your bank's
 * forms.
 */
const SAMPLES: { file: string; label: string; hint: string }[] = [
  { file: '05-cheque.png', label: 'cheque', hint: 'CAR/LAR agree · 7025' },
  { file: '04-rtgs.png', label: 'rtgs', hint: 'CAR/LAR across paise · 12500.50' },
  {
    file: '06-car-skew-invalid.png',
    label: 'car · skewed',
    hint: 'tilted page · impossible date, faulted',
  },
];

/** `Profile::describes()` from crates/nayen-core/src/params.rs, verbatim. */
const PROFILE_DESC: Record<Profile, string> = {
  indra: 'reference pipeline: thresh 0.5, min_area 2, no box growth, pad_y 0.2',
  handwriting:
    'handwriting candidate: thresh 0.5, min_area 1, grow 0.12/0.08, pad_y 0.35, contrast stretch',
  sweep: 'multi-scale sweep: thresh 0.45, no adaptive pass, merged boxes, pad_y 0.2',
};

/** EngineConfig::browser — same maths, lower memory ceiling. */
const MAX_PAGE_SIDE = 1800;
/** pipeline.MIN_CONF */
const MIN_CONF = 0.5;
/** DESIGN.md §9 safe bound: reject anything above 25 megapixels. */
const MAX_PIXELS = 25_000_000;

const ACCEPTED = ['image/png', 'image/jpeg', 'image/webp'];

type LineState = 'ok' | 'review' | 'none';

function lineState(line: LineResult): LineState {
  if (line.text === '') return 'none';
  if (line.needsReview || line.conf < 0.8) return 'review';
  return 'ok';
}

const STATE_LABEL: Record<LineState, string> = {
  ok: 'verified',
  review: 'needs review',
  none: 'no text',
};

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

function $(id: string): HTMLElement | null {
  return document.getElementById(id);
}

export function initDemo(info: DriverInfo): void {
  const dropzone = $('dropzone');
  const fileInput = $('file-input') as HTMLInputElement | null;
  const fileNameEl = $('file-name');
  const runBtn = $('run-btn') as HTMLButtonElement | null;
  const imgEl = $('demo-img') as HTMLImageElement | null;
  const overlay = $('demo-overlay') as unknown as SVGSVGElement | null;
  const viewerEmpty = $('viewer-empty');
  const watermark = $('viewer-watermark');
  const simflag = $('viewer-simflag');
  const telog = $('telog');
  const resultsRoot = $('demo-results');
  const linesBody = $('lines-body');
  const linesCount = $('lines-count');
  const timingsEl = $('timings');
  const fieldsEl = $('fields');
  const exportMd = $('export-md') as HTMLButtonElement | null;
  const exportJson = $('export-json') as HTMLButtonElement | null;
  const exportNote = $('export-note');
  const profileDesc = $('profile-desc');
  const demoSection = $('demo');

  if (
    !dropzone ||
    !fileInput ||
    !runBtn ||
    !imgEl ||
    !overlay ||
    !telog ||
    !resultsRoot ||
    !linesBody ||
    !timingsEl ||
    !fieldsEl ||
    !exportMd ||
    !exportJson ||
    !demoSection
  ) {
    return;
  }

  let imageData: ImageData | null = null;
  let objectUrl: string | null = null;
  let currentName = '';
  let result: PageResult | null = null;
  let config: EngineConfig | null = null;
  let running = false;

  /* ------------------------------------------------------------ telemetry */

  const log = (parts: { text: string; cls?: string }[]): void => {
    const li = el('li');
    for (const part of parts) li.append(el('span', part.cls, part.text));
    telog.append(li);
    telog.scrollTop = telog.scrollHeight;
  };

  const clearLog = (): void => {
    telog.replaceChildren();
  };

  const logEvent = (e: ProgressEvent): void => {
    switch (e.stage) {
      case 'decode-image':
        log([{ text: 'decode-image ', cls: 't-stage' }, { text: '· local canvas, no upload', cls: 't-dim' }]);
        break;
      case 'orient':
        log([
          { text: 'orient ', cls: 't-stage' },
          {
            text: e.degrees === 0 ? '· quarter-turn check' : `· applied ${e.degrees}°`,
            cls: 't-dim',
          },
        ]);
        break;
      case 'detect':
        log([
          { text: 'detect ', cls: 't-stage' },
          {
            text:
              e.lines > 0
                ? `· ${e.lines} lines · pass ${e.pass}`
                : `· pass ${e.pass}`,
            cls: 't-dim',
          },
        ]);
        break;
      case 'route': {
        const g = e.groups;
        const total = g.base + g.cjk + g.rtl + g.other;
        log([
          { text: 'route ', cls: 't-stage' },
          {
            text:
              total > 0
                ? `· base ${g.base} · cjk ${g.cjk} · rtl ${g.rtl} · other ${g.other}`
                : '· script classifier ran',
            cls: 't-dim',
          },
        ]);
        break;
      }
      case 'recognize':
        log([
          { text: 'recognize ', cls: 't-stage' },
          { text: `· batch ${e.batch}${e.of > e.batch ? `/${e.of}` : ''}`, cls: 't-dim' },
        ]);
        break;
      case 'laya':
        log([
          { text: 'laya ', cls: 't-stage' },
          { text: e.fields > 0 ? `· ${e.fields} fields` : '· decisions', cls: 't-dim' },
        ]);
        break;
      case 'done':
        log([{ text: 'done', cls: 't-stage' }]);
        break;
    }
  };

  /* --------------------------------------------------------------- config */

  const readConfig = (): EngineConfig => ({
    profile: (document.querySelector<HTMLInputElement>('input[name="profile"]:checked')
      ?.value ?? 'indra') as Profile,
    minConf: MIN_CONF,
    orient: (document.getElementById('opt-orient') as HTMLInputElement | null)?.checked ?? true,
    routeScripts:
      (document.getElementById('opt-route') as HTMLInputElement | null)?.checked ?? true,
    laya: (document.getElementById('opt-laya') as HTMLInputElement | null)?.checked ?? true,
    maxPageSide: MAX_PAGE_SIDE,
  });

  document.querySelectorAll<HTMLInputElement>('input[name="profile"]').forEach((radio) => {
    radio.addEventListener('change', () => {
      if (profileDesc) profileDesc.textContent = PROFILE_DESC[radio.value as Profile] ?? '';
    });
  });

  /* ------------------------------------------------------------ image in */

  const decode = async (file: File): Promise<void> => {
    if (!ACCEPTED.includes(file.type)) {
      log([{ text: `rejected ${file.type || 'unknown type'} `, cls: 't-err' }, { text: '· PNG/JPEG/WebP only', cls: 't-dim' }]);
      return;
    }
    try {
      const bitmap = await createImageBitmap(file);
      if (bitmap.width * bitmap.height > MAX_PIXELS) {
        bitmap.close();
        log([{ text: 'rejected ', cls: 't-err' }, { text: `· above ${MAX_PIXELS / 1e6} MP safe bound`, cls: 't-dim' }]);
        return;
      }
      const scale = Math.min(1, MAX_PAGE_SIDE / Math.max(bitmap.width, bitmap.height));
      const w = Math.max(1, Math.round(bitmap.width * scale));
      const h = Math.max(1, Math.round(bitmap.height * scale));
      const canvas = document.createElement('canvas');
      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext('2d', { willReadFrequently: true });
      if (!ctx) throw new Error('canvas 2d context unavailable');
      ctx.drawImage(bitmap, 0, 0, w, h);
      bitmap.close();
      imageData = ctx.getImageData(0, 0, w, h);

      if (objectUrl) URL.revokeObjectURL(objectUrl);
      objectUrl = URL.createObjectURL(file);
      imgEl.src = objectUrl;
      imgEl.alt = `Your scan: ${file.name}`;
      imgEl.hidden = false;
      overlay.replaceChildren();
      overlay.removeAttribute('hidden');
      if (viewerEmpty) viewerEmpty.hidden = true;
      currentName = file.name;
      if (fileNameEl) {
        fileNameEl.textContent = `${file.name} · ${imageData.width}×${imageData.height}px after bound`;
        fileNameEl.hidden = false;
      }
      // Reset results until a new run completes.
      result = null;
      resultsRoot.hidden = true;
      if (watermark) watermark.hidden = true;
      if (simflag) simflag.hidden = true;
      clearLog();
      log([{ text: `page loaded · ${file.name}`, cls: 't-dim' }]);
      runBtn.disabled = false;
      runBtn.focus();
    } catch (err) {
      log([{ text: 'decode failed ', cls: 't-err' }, { text: `· ${String(err)}`, cls: 't-dim' }]);
    }
  };

  fileInput.addEventListener('change', () => {
    const file = fileInput.files?.[0];
    if (file) void decode(file);
    fileInput.value = '';
  });

  /* ------------------------------------------------------ shipped samples */

  /** Fetch a shipped sample and present it to `decode` exactly as a dropped file. */
  const loadSample = async (name: string): Promise<File | null> => {
    try {
      const res = await fetch(assetUrl(`samples/${name}`));
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      return new File([await res.blob()], name, { type: 'image/png' });
    } catch (err) {
      log([
        { text: 'sample unavailable ', cls: 't-err' },
        { text: `· ${name}: ${String(err)}`, cls: 't-dim' },
      ]);
      return null;
    }
  };

  const samplesRow = $('samples-row');
  if (samplesRow) {
    for (const sample of SAMPLES) {
      const button = el('button', 'samples__btn');
      button.type = 'button';
      button.append(
        el('span', 'samples__name', sample.label),
        el('span', 'samples__hint', sample.hint),
      );
      button.addEventListener('click', () => {
        if (running) return;
        button.disabled = true;
        void loadSample(sample.file)
          .then((file) => (file ? decode(file) : undefined))
          .finally(() => {
            button.disabled = false;
          });
      });
      samplesRow.append(button);
    }
  }

  const setOver = (on: boolean): void => {
    dropzone.classList.toggle('is-over', on);
  };

  ['dragenter', 'dragover'].forEach((type) => {
    dropzone.addEventListener(type, (event) => {
      event.preventDefault();
      setOver(true);
    });
  });
  ['dragleave', 'drop'].forEach((type) => {
    dropzone.addEventListener(type, (event) => {
      event.preventDefault();
      setOver(false);
    });
  });
  dropzone.addEventListener('drop', (event) => {
    const file = (event as DragEvent).dataTransfer?.files?.[0];
    if (file) void decode(file);
  });
  // Never let a missed drop navigate away from the page.
  window.addEventListener('dragover', (event) => event.preventDefault());
  window.addEventListener('drop', (event) => event.preventDefault());

  // Paste works only while the demo section is on screen.
  let demoVisible = false;
  new IntersectionObserver(
    (entries) => {
      demoVisible = entries.some((entry) => entry.isIntersecting);
    },
    { threshold: 0.15 },
  ).observe(demoSection);

  window.addEventListener('paste', (event) => {
    if (!demoVisible || running) return;
    const items = event.clipboardData?.items;
    if (!items) return;
    for (const item of items) {
      if (item.type.startsWith('image/')) {
        const file = item.getAsFile();
        if (file) {
          event.preventDefault();
          void decode(file);
        }
        return;
      }
    }
  });

  /* ------------------------------------------------------------------ run */

  const renderOverlay = (page: PageResult): void => {
    overlay.replaceChildren();
    overlay.setAttribute('viewBox', `0 0 ${page.width} ${page.height}`);
    overlay.setAttribute('preserveAspectRatio', 'none');
    page.lines.forEach((line, index) => {
      const state = lineState(line);
      const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
      rect.setAttribute('x', String(line.box.x0));
      rect.setAttribute('y', String(line.box.y0));
      rect.setAttribute('width', String(Math.max(line.box.x1 - line.box.x0, 1)));
      rect.setAttribute('height', String(Math.max(line.box.y1 - line.box.y0, 1)));
      rect.dataset.idx = String(index);
      if (state === 'review') rect.classList.add('is-review');
      if (state === 'none') rect.classList.add('is-empty');
      overlay.append(rect);
    });
  };

  const renderLines = (page: PageResult): void => {
    linesBody.replaceChildren();
    if (linesCount) {
      const kept = page.lines.filter((l) => l.text !== '').length;
      linesCount.textContent = `${page.lines.length} detected · ${kept} with text`;
    }
    page.lines.forEach((line, index) => {
      const state = lineState(line);
      const tr = el('tr');
      tr.dataset.idx = String(index);
      tr.tabIndex = 0;

      const tdIdx = el('td', 'c-idx', String(index).padStart(2, '0'));
      const tdText = el(
        'td',
        state === 'none' ? 'c-text c-text--none' : 'c-text',
        line.text || `(no text — below MIN_CONF ${MIN_CONF})`,
      );

      const tdConf = el('td', 'c-conf');
      tdConf.append(el('span', 'conf-val', line.conf.toFixed(2)));
      const bar = el('span', 'bar');
      const fill = el(
        'span',
        `bar__fill ${state === 'none' ? 'bar__fill--bad' : state === 'review' ? 'bar__fill--review' : 'bar__fill--good'}`,
      );
      fill.style.setProperty('--w', `${Math.round(line.conf * 100)}%`);
      // Show immediately (row is rendered post-run, no reveal gate here).
      fill.style.width = `${Math.round(line.conf * 100)}%`;
      bar.append(fill);
      tdConf.append(bar);

      const tdState = el('td');
      tdState.append(el('span', `state state--${state}`, STATE_LABEL[state]));

      tr.append(tdIdx, tdText, tdConf, tdState);
      linesBody.append(tr);
    });
  };

  const renderTimings = (page: PageResult): void => {
    timingsEl.replaceChildren();
    const rows: [string, number][] = [
      ['orient', page.timings.orient],
      ['detect', page.timings.detect],
      ['crop', page.timings.crop],
      ['recognize', page.timings.recognize],
      ['laya', page.timings.laya],
      ['total', page.timings.total],
    ];
    for (const [name, ms] of rows) {
      timingsEl.append(el('dt', '', name));
      timingsEl.append(el('dd', '', `${ms.toFixed(1)} ms`));
    }
    timingsEl.append(el('dt', '', 'models'));
    timingsEl.append(
      el('dd', '', page.modelsUsed.length ? page.modelsUsed.map((m) => `${m}.onnx`).join(' ') : '— none (simulated)'),
    );
  };

  const renderFields = (page: PageResult): void => {
    fieldsEl.replaceChildren();
    const fx = page.fields;
    if (!fx) {
      fieldsEl.append(el('p', 'fields__empty', 'Laya disabled for this run'));
      return;
    }
    if (fx.docType) {
      const dt = el('p', 'fields__empty', `docType: ${fx.docType}`);
      fieldsEl.append(dt);
    }
    for (const field of fx.fields) {
      const card = el('div', 'field');
      const top = el('div', 'field__top');
      top.append(el('span', 'field__name', field.name));
      const validCls =
        field.valid === true ? 'valid--true' : field.valid === false ? 'valid--false' : 'valid--null';
      const validTxt =
        field.valid === true ? 'valid' : field.valid === false ? 'invalid' : 'not found';
      top.append(el('span', `state ${validCls}`, `${validTxt} · ${field.conf.toFixed(2)}`));
      card.append(top);
      card.append(el('p', 'field__value', field.value || '—'));
      card.append(el('p', 'field__rule', field.rule));
      fieldsEl.append(card);
    }
    if (fx.carLarMatch !== null) {
      const cls = fx.carLarMatch ? 'valid--true' : 'valid--false';
      const row = el('p', `field__rule ${cls}`, `CAR ↔ LAR cross-check: ${fx.carLarMatch ? 'match' : 'MISMATCH'}`);
      fieldsEl.append(row);
    }
    for (const anomaly of fx.anomalies) {
      fieldsEl.append(
        el('p', `anomaly anomaly--${anomaly.severity}`, `${anomaly.code}: ${anomaly.message}`),
      );
    }
  };

  // Row ↔ box hover/focus sync.
  const setActive = (index: string | null, on: boolean): void => {
    if (index === null) return;
    overlay.querySelector(`rect[data-idx="${index}"]`)?.classList.toggle('is-active', on);
    linesBody.querySelector(`tr[data-idx="${index}"]`)?.classList.toggle('is-active', on);
  };

  linesBody.addEventListener('pointerover', (event) => {
    const row = (event.target as HTMLElement).closest('tr');
    if (row?.dataset.idx) setActive(row.dataset.idx, true);
  });
  linesBody.addEventListener('pointerout', (event) => {
    const row = (event.target as HTMLElement).closest('tr');
    if (row?.dataset.idx) setActive(row.dataset.idx, false);
  });
  linesBody.addEventListener('focusin', (event) => {
    const row = (event.target as HTMLElement).closest('tr');
    if (row?.dataset.idx) setActive(row.dataset.idx, true);
  });
  linesBody.addEventListener('focusout', (event) => {
    const row = (event.target as HTMLElement).closest('tr');
    if (row?.dataset.idx) setActive(row.dataset.idx, false);
  });

  runBtn.addEventListener('click', async () => {
    if (!imageData || running) return;
    running = true;
    runBtn.disabled = true;
    runBtn.textContent = 'Running…';
    clearLog();
    log([
      { text: `driver ${info.label} `, cls: info.driver.simulated ? 't-warn' : 't-stage' },
      { text: `· ${info.driver.simulated ? 'simulation — engine not loaded' : 'live inference'}`, cls: 't-dim' },
    ]);
    config = readConfig();
    log([{ text: `config · profile ${config.profile} · minConf ${config.minConf}`, cls: 't-dim' }]);
    try {
      result = await info.driver.run(imageData, config, logEvent);
      renderOverlay(result);
      renderLines(result);
      renderTimings(result);
      renderFields(result);
      resultsRoot.hidden = false;
      exportMd.disabled = false;
      exportJson.disabled = false;
      if (info.driver.simulated) {
        if (watermark) watermark.hidden = false;
        if (simflag) simflag.hidden = false;
        if (exportNote) {
          exportNote.textContent = 'exports contain simulated data — labelled as such inside';
        }
      } else {
        if (watermark) watermark.hidden = true;
        if (simflag) simflag.hidden = true;
        if (exportNote) exportNote.textContent = '';
      }
    } catch (err) {
      log([{ text: 'run failed ', cls: 't-err' }, { text: `· ${String(err)}`, cls: 't-dim' }]);
    } finally {
      running = false;
      runBtn.disabled = !imageData;
      runBtn.textContent = 'Run engine';
    }
  });

  /* -------------------------------------------------------------- exports */

  const download = (content: string, mime: string, filename: string): void => {
    const blob = new Blob([content], { type: mime });
    const url = URL.createObjectURL(blob);
    const a = el('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  };

  const stamp = (): string => new Date().toISOString().replaceAll(':', '-').slice(0, 19);

  exportMd.disabled = true;
  exportJson.disabled = true;

  exportMd.addEventListener('click', () => {
    if (!result) return;
    const provenance = [
      '',
      '---',
      `driver: ${info.label}${info.driver.simulated ? ' (SIMULATED — not OCR output)' : ''}`,
      `profile: ${result.profile} · engine: ${info.engineVersion ?? 'not loaded'}`,
      `models used: ${result.modelsUsed.length ? result.modelsUsed.join(', ') : 'none'}`,
      `source file: ${currentName}`,
    ].join('\n');
    download(result.markdown + provenance, 'text/markdown;charset=utf-8', `nayen-v1-${result.profile}-${stamp()}.md`);
  });

  exportJson.addEventListener('click', () => {
    if (!result || !config) return;
    const payload = {
      engine: 'nayen-v1',
      driver: {
        name: info.driver.name,
        simulated: info.driver.simulated,
        engineVersion: info.engineVersion,
        modelsDetected: info.modelsPresent,
        modelsMissing: info.modelsMissing,
      },
      config,
      sourceFile: currentName,
      exportedAt: new Date().toISOString(),
      result,
    };
    download(JSON.stringify(payload, null, 2), 'application/json', `nayen-v1-${result.profile}-${stamp()}.json`);
  });
}
