/**
 * NAYEN v1 documentation site generator (GitHub Pages).
 *
 * Publishes exactly two documentation areas — SDK (the NAYEN v1 engine) and
 * Studio (the operator workspace). Marketing/landing content intentionally
 * lives elsewhere (the Vercel product site in web/); this surface is docs only.
 *
 * Design tokens are consumed from packages/theme/tokens.css (copied into the
 * output, never re-declared here). Build must keep working under:
 *   npm ci --ignore-scripts && npm run build   →   docs/site/dist
 */
import { createHash } from 'node:crypto';
import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Marked } from 'marked';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const out = path.join(here, 'dist');
const project = 'https://github.com/abhays02/ICR-Studios';
const repository = `${project}/blob/main/`;
// Live product site (web/, deployed on Vercel) and its demo section.
// Override with NAYEN_SITE_URL when the deployment address changes.
const productSite = process.env.NAYEN_SITE_URL || 'https://icr-studios.vercel.app';
const demoSite = `${productSite}/#demo`;

const escape = (value) => String(value).replace(/[&<>"']/g, (char) => ({
  '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
}[char]));
const inlineText = (tokens) => tokens.map((token) => token.type === 'html' ? ''
  : token.tokens ? inlineText(token.tokens) : token.text ?? token.raw).join('');
const slug = (value) => value.toLowerCase()
  .replace(/[^\p{L}\p{N}_\s-]/gu, '').trim().replace(/\s/g, '-');

/* ------------------------------------------------------------------ registry
   The complete published surface. Anything not listed here is not published
   from Pages (it stays in the repo; links resolve to GitHub instead). */
const REGISTRY = [
  { area: 'sdk', source: 'docs/site/content/sdk/architecture.md', output: 'sdk-architecture.html', label: 'Architecture' },
  { area: 'sdk', source: 'docs/site/content/sdk/host-protocol.md', output: 'sdk-host-protocol.html', label: 'Host protocol & executor seam' },
  { area: 'sdk', source: 'docs/site/content/sdk/rust-api.md', output: 'sdk-rust-api.html', label: 'Rust crate API' },
  { area: 'sdk', source: 'docs/site/content/sdk/wasm.md', output: 'sdk-wasm.html', label: 'WASM integration' },
  { area: 'sdk', source: 'docs/site/content/sdk/profiles.md', output: 'sdk-profiles.html', label: 'Profiles & parameters' },
  { area: 'sdk', source: 'docs/MODELS.md', output: 'sdk-provenance.html', label: 'Model provenance & licensing' },
  { area: 'studio', source: 'docs/STUDIO.md', output: 'studio-guide.html', label: 'Operator guide' },
  { area: 'studio', source: 'docs/API.md', output: 'studio-api.html', label: 'Studio API contract' },
  { area: 'studio', source: 'docs/site/content/studio/windows.md', output: 'studio-windows.html', label: 'Windows build & run' },
  { area: 'studio', source: 'data/samples/README.md', output: 'studio-fixtures.html', label: 'Synthetic fixtures' },
  { area: 'studio', source: 'studio/README.md', output: 'studio-development.html', label: 'Studio development' },
];
const AREAS = {
  sdk: { name: 'SDK', blurb: 'The NAYEN v1 engine: Indra perception, Laya decisions, the executor seam, WASM host loop, profiles and model provenance.' },
  studio: { name: 'Studio', blurb: 'The operator workspace: unlock, import, inspect, correct, approve, export — and the Windows package.' },
};

/* ------------------------------------------------------- contract excerpts
   %%include <repo-file>#<Symbol>%% pulls the real definition from source at
   build time so SDK pages cannot drift from engine/src/types.ts. */
function extractSymbol(file, symbol) {
  const lines = file.split('\n');
  const start = lines.findIndex((line) =>
    new RegExp(`^export (interface|type|const) ${symbol}\\b`).test(line));
  if (start < 0) throw new Error(`Contract symbol not found: ${symbol}`);
  let begin = start;
  while (begin > 0 && /^\s*(\/\*\*|\*|\/\/)/.test(lines[begin - 1])) begin -= 1;
  let depth = 0;
  let end = start;
  for (; end < lines.length; end += 1) {
    for (const char of lines[end]) {
      if ('{[('.includes(char)) depth += 1;
      if ('}])'.includes(char)) depth -= 1;
    }
    if (end > start && depth <= 0) break;
    if (end === start && depth === 0 && lines[end].trimEnd().endsWith(';')) break;
  }
  return lines.slice(begin, end + 1).join('\n');
}

async function applyIncludes(markdown) {
  const cache = new Map();
  const parts = markdown.split('\n');
  for (let index = 0; index < parts.length; index += 1) {
    const match = parts[index].match(/^%%include\s+(\S+)#(\w+)%%$/);
    if (!match) continue;
    const [, file, symbol] = match;
    if (!cache.has(file)) cache.set(file, await readFile(path.join(root, file), 'utf8'));
    parts[index] = `\`\`\`ts\n${extractSymbol(cache.get(file), symbol)}\n\`\`\``;
  }
  return parts.join('\n');
}

/* ------------------------------------------------------------- highlighting
   Small hand-rolled tokenizer — no runtime dependency, deterministic output. */
const KEYWORDS = {
  ts: ['as','async','await','boolean','break','case','catch','class','const','continue','default','do','else','enum','export','extends','false','for','from','function','get','if','implements','import','in','instanceof','interface','keyof','let','new','null','number','of','private','public','readonly','return','satisfies','set','static','string','super','switch','this','throw','true','try','type','typeof','undefined','unique','unknown','var','void','while','yield'],
  rust: ['as','async','await','bool','break','const','continue','crate','dyn','else','enum','false','fn','for','f32','f64','i64','i32','u8','u32','usize','if','impl','in','let','loop','match','mod','move','mut','pub','ref','return','self','Self','static','str','struct','super','trait','true','type','unsafe','use','where','while'],
  py: ['and','as','assert','async','await','break','class','continue','def','del','elif','else','except','False','finally','for','from','global','if','import','in','is','lambda','None','nonlocal','not','or','pass','raise','return','True','try','while','with','yield'],
  sh: ['cd','echo','exec','exit','export','if','then','fi','for','in','npm','npx','node','python','python3','set','uv','git','curl','magick','rsvg-convert'],
  bash: [],
  json: ['true','false','null'],
  yaml: ['true','false','null','on','off'],
  toml: ['true','false'],
};
function highlight(code, rawLang) {
  const lang = String(rawLang || '').toLowerCase();
  const keywords = KEYWORDS[lang] ?? KEYWORDS[lang === 'typescript' ? 'ts' : lang === 'python' ? 'py' : lang === 'shell' ? 'sh' : ''] ?? [];
  const hashComments = ['py', 'python', 'sh', 'bash', 'shell', 'yaml', 'toml'].includes(lang);
  const slashComments = !hashComments && lang !== 'json';
  const patterns = [];
  patterns.push(/\/\*[\s\S]*?\*\/(?:\/)?/);            // block comments
  if (slashComments) patterns.push(/\/\/[^\n]*/);
  if (hashComments) patterns.push(/#[^\n]*/);
  patterns.push(/"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\[\s\S])*`/); // strings
  patterns.push(/\b0[xX][\da-fA-F_]+\b|\b\d[\d_]*(?:\.\d+)?\b/);                   // numbers
  if (keywords.length) patterns.push(new RegExp(`\\b(?:${keywords.join('|')})\\b`));
  patterns.push(/\b[A-Z][A-Za-z0-9_]*\b/);              // type-like identifiers
  const combined = new RegExp(`(${patterns.map((p) => p.source).join(')|(')})`, 'g');
  const classes = ['hl-c'];                               // block comment
  if (slashComments) classes.push('hl-c');
  if (hashComments) classes.push('hl-c');
  classes.push('hl-s', 'hl-n');
  if (keywords.length) classes.push('hl-k');
  classes.push('hl-t');
  let html = '';
  let cursor = 0;
  for (const match of code.matchAll(combined)) {
    html += escape(code.slice(cursor, match.index));
    const groupIndex = match.slice(1).findIndex((part) => part !== undefined);
    const cls = classes[groupIndex] ?? 'hl-t';
    html += `<span class="${cls}">${escape(match[0])}</span>`;
    cursor = match.index + match[0].length;
  }
  return html + escape(code.slice(cursor));
}

/* -------------------------------------------------------------------- pages */
const localLinks = [];
const pageHeadings = new Map();
const bySource = new Map();
const pages = await Promise.all(REGISTRY.map(async (entry) => {
  const raw = await readFile(path.join(root, entry.source), 'utf8');
  const markdown = await applyIncludes(raw);
  const title = markdown.match(/^#\s+(.+)$/m)?.[1]?.replace(/[`*]/g, '') ?? entry.label;
  const excerpt = (markdown.replace(/^#\s+.+$/m, '')
    .replace(/```[\s\S]*?```/g, ' ')
    .split(/\n\s*\n/)
    .map((paragraph) => paragraph.replace(/^\s*>/gm, ' ')
      .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1').replace(/[*_`#|]/g, ' ')
      .replace(/\s+/g, ' ').trim())
    .find((paragraph) => paragraph.length >= 80) ?? '').slice(0, 200);
  const page = { ...entry, markdown, title, excerpt, headings: [] };
  bySource.set(entry.source, page);
  return page;
}));
// Repo sources that are published under a different area also resolve.
const outputs = new Map(pages.map((page) => [page.output, page]));

function resolveURL(href, page, image = false) {
  if (/^(https?:|mailto:)/i.test(href)) return image ? '' : href;
  if (/^[a-z][a-z\d+.-]*:|^[/\\]{2}/i.test(href)) return '';
  const match = href.match(/^([^?#]*)([?#].*)?$/);
  const pathname = decodeURIComponent(match[1]);
  const suffix = match[2] ?? '';
  if (pathname.endsWith('.html')) {
    if (outputs.has(path.posix.basename(pathname))) return path.posix.basename(pathname) + suffix;
    return image ? '' : href;
  }
  const source = pathname ? path.posix.normalize(pathname.startsWith('/') ? pathname.slice(1)
    : path.posix.join(path.posix.dirname(page.source), pathname)) : page.source;
  if (bySource.has(source)) {
    localLinks.push({ source, suffix, from: page.source });
    return bySource.get(source).output + suffix;
  }
  if (source.startsWith('docs/media/')) return `media/${path.posix.basename(source)}${suffix}`;
  if (image) return '';
  return repository + source.split('/').map(encodeURIComponent).join('/') + suffix;
}

/* ------------------------------------------------------------------ chrome */
const THEME_SCRIPT = `(function(){try{var t=localStorage.getItem('nayen-docs-theme');if(t!=='light'&&t!=='dark'){t=window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light';}document.documentElement.setAttribute('data-theme',t);}catch(e){document.documentElement.setAttribute('data-theme','light');}})();`;
const themeHash = `'sha256-${createHash('sha256').update(THEME_SCRIPT).digest('base64')}'`;

const BRAND_SVG = `<svg class="brand-svg" viewBox="0 0 48 48" aria-hidden="true"><rect x="1" y="1" width="46" height="46" rx="9" fill="var(--ink-850)" stroke="var(--border-strong)"/><g fill="var(--ink-200)"><rect x="9" y="13" width="21" height="3" rx="1.5"/><rect x="9" y="22.5" width="30" height="3" rx="1.5"/><rect x="9" y="32" width="15" height="3" rx="1.5"/></g><rect x="33.5" y="7" width="2.5" height="34" rx="1.25" fill="var(--signal-500)"/><circle cx="34.75" cy="24" r="3.4" fill="none" stroke="var(--signal-400)" stroke-width="1.6"/></svg>`;

function head(title, description) {
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<script>${THEME_SCRIPT}</script>
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self' ${themeHash}; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'; object-src 'none'">
<meta name="description" content="${escape(description)}">
<meta name="theme-color" content="#0b0d10">
<title>${escape(title)} · NAYEN v1 docs</title>
<link rel="icon" type="image/svg+xml" href="nayen-mark.svg">
<link rel="stylesheet" href="tokens.css"><link rel="stylesheet" href="styles.css">
<script src="index-data.js" defer></script><script src="app.js" defer></script></head>`;
}

function header(active) {
  return `<a class="skip" href="#content">Skip to content</a>
<header class="site-header">
<a class="brand" href="index.html" aria-label="NAYEN v1 documentation home">${BRAND_SVG}<span class="brand-name">NAYEN<span class="brand-v">v1</span></span><span class="brand-docs">SDK &amp; STUDIO DOCS</span></a>
<nav aria-label="Main navigation">
<a href="index.html#area-sdk"${active === 'sdk' ? ' aria-current="true"' : ''}>SDK</a>
<a href="index.html#area-studio"${active === 'studio' ? ' aria-current="true"' : ''}>Studio</a>
<a class="ext" href="${productSite}">Product site <span aria-hidden="true">↗</span></a>
<a class="ext" href="${demoSite}">Live demo <span aria-hidden="true">↗</span></a>
<button id="theme-toggle" type="button" aria-pressed="false"><span class="toggle-label">Dark</span></button>
<a class="ext github-link" href="${project}">GitHub <span aria-hidden="true">↗</span></a>
</nav></header>`;
}

function sidebar(page) {
  const group = (area) => `<div class="nav-group" data-area="${area}">
<p class="nav-area">${AREAS[area].name} documentation</p>
<ul>${pages.filter((entry) => entry.area === area).map((entry) =>
    `<li><a href="${entry.output}" data-search="${escape(`${entry.title} ${entry.label} ${entry.source}`.toLowerCase())}"${entry === page ? ' aria-current="page"' : ''}>${escape(entry.label)}</a></li>`).join('')}
</ul></div>`;
  return `<aside class="sidebar">
<label class="filter-label" for="nav-filter">Filter pages</label>
<input id="nav-filter" type="search" placeholder="Filter SDK & Studio pages…" autocomplete="off" aria-describedby="nav-filter-hint">
<p id="nav-filter-hint" class="nav-hint" role="status">${pages.length} pages · offline</p>
<nav class="side-nav" aria-label="Documentation areas">${group('sdk')}${group('studio')}</nav>
<p class="no-match" role="status" hidden>No matching pages.</p>
<div class="side-links">
<a href="${productSite}">Live product site ↗</a>
<a href="${demoSite}">Live engine demo ↗</a>
<a href="${project}/releases">Windows downloads ↗</a>
</div>
<p class="sidebar-note">NAYEN v1 · ICR STUDIOS<br>Manual-review prototype. No bundled weights,<br>no validated OCR accuracy claim.</p>
</aside>`;
}

function toc(headings) {
  const items = headings.filter(({ depth }) => depth === 2 || depth === 3);
  if (!items.length) return '';
  return `<aside class="toc" aria-label="On this page"><p class="toc-title">On this page</p><nav><ul>${items.map(({ depth, text, id }) =>
    `<li class="toc-d${depth}"><a href="#${escape(id)}" data-spy="${escape(id)}">${escape(text)}</a></li>`).join('')}</ul></nav></aside>`;
}

function pager(page) {
  const index = pages.indexOf(page);
  const prev = pages[index - 1];
  const next = pages[index + 1];
  return `<nav class="pager" aria-label="Page navigation">
${prev ? `<a class="pager-prev" href="${prev.output}" rel="prev"><span>← Previous</span><strong>${escape(prev.title)}</strong><small>${AREAS[prev.area].name} docs</small></a>` : '<span></span>'}
${next ? `<a class="pager-next" href="${next.output}" rel="next"><span>Next →</span><strong>${escape(next.title)}</strong><small>${AREAS[next.area].name} docs</small></a>` : '<span></span>'}
</nav>`;
}

function footer(page) {
  return `<footer class="site-footer">${BRAND_SVG}
<div><p class="footer-brand">NAYEN<span>v1</span> · ICR STUDIOS</p>
<p>Evidence first. Human authority, always. Synthetic illustrations only.</p></div>
<a href="${page ? `${project}/edit/main/${page.source.split('/').map(encodeURIComponent).join('/')}` : project}">${page ? 'Edit this page on GitHub ↗' : 'Explore the repository ↗'}</a>
<span>Static build · no runtime network calls · ${page ? `<a href="${repository + page.source.split('/').map(encodeURIComponent).join('/')}">Markdown source ↗</a>` : ''}</span></footer>`;
}

/* ----------------------------------------------------------------- markdown */
function renderMarkdown(page) {
  const headings = [];
  const duplicates = new Map();
  const markdown = new Marked({
    renderer: {
      html({ text }) {
        return text.split(/(<\/?(?:details|summary)\s*>)/i).map((part) =>
          /^<\/?(?:details|summary)\s*>$/i.test(part) ? part : escape(part)).join('');
      },
      heading({ tokens, depth }) {
        const text = this.parser.parseInline(tokens);
        const plainText = inlineText(tokens);
        const base = slug(plainText);
        const count = duplicates.get(base) ?? 0;
        duplicates.set(base, count + 1);
        const id = base + (count ? `-${count}` : '');
        headings.push({ depth, text: plainText, id });
        return `<h${depth} id="${escape(id)}">${text}<a class="anchor" href="#${escape(id)}" aria-label="Link to section: ${escape(plainText)}">#</a></h${depth}>`;
      },
      link({ href, tokens, title }) {
        const url = resolveURL(href, page);
        return `<a href="${escape(url)}"${title ? ` title="${escape(title)}"` : ''}>${this.parser.parseInline(tokens)}</a>`;
      },
      image({ href, text }) {
        const url = resolveURL(href, page, true);
        if (!url) return `<span class="image-alternative">${escape(text)}</span>`;
        if (url === 'media/studio-walkthrough.gif') {
          return `<figure class="recording"><img src="media/studio-review.png" data-recording="${escape(url)}" alt="${escape(text)}" loading="lazy"><button class="recording-toggle" type="button" aria-pressed="false" hidden>Play recorded Studio demo</button><figcaption>Real synthetic-only recording. Playback is optional; stop returns to a still image.</figcaption></figure>`;
        }
        return `<img src="${escape(url)}" alt="${escape(text)}" loading="lazy">`;
      },
      code({ text, lang }) {
        const language = String(lang || '').trim();
        return `<pre class="code"${language ? ` data-lang="${escape(language)}"` : ''}><code>${highlight(text, language)}</code></pre>`;
      },
    },
  });
  const content = markdown.parse(page.markdown);
  pageHeadings.set(page.source, new Set(headings.map(({ id }) => id)));
  page.headings = headings;
  return content;
}

function renderPage(page) {
  const content = renderMarkdown(page);
  return `${head(page.title, `${page.title} — NAYEN v1 ${AREAS[page.area].name} documentation for ICR Studios.`)}
<body class="doc-page">${header(page.area)}<div class="shell">${sidebar(page)}
<main id="content">
<p class="crumb"><a href="index.html">Docs</a> / <span>${AREAS[page.area].name}</span> / ${escape(page.label)}</p>
<article class="doc">${content}</article>
${pager(page)}
${footer(page)}
</main>${toc(page.headings)}</div></body></html>`;
}

/* --------------------------------------------------------------------- home */
function home() {
  const areaSection = (area) => {
    const entries = pages.filter((page) => page.area === area);
    return `<section class="area" id="area-${area}" aria-labelledby="area-${area}-title">
<div class="area-head"><div><p class="eyebrow">${String(area).toUpperCase()} DOCUMENTATION</p>
<h2 id="area-${area}-title">${AREAS[area].name}</h2></div>
<span class="area-count">${entries.length} pages</span></div>
<p class="area-blurb">${escape(AREAS[area].blurb)}</p>
<ol class="page-list">${entries.map((page, index) =>
    `<li><a href="${page.output}"><span class="page-no">${String(index + 1).padStart(2, '0')}</span><span class="page-body"><strong>${escape(page.label)}</strong><small>${escape(page.excerpt)}</small></span><span class="page-go" aria-hidden="true">→</span></a></li>`).join('')}</ol>
</section>`;
  };
  return `${head('NAYEN v1 documentation', 'SDK and Studio documentation for NAYEN v1 — the ICR Studios handwriting-form ICR engine (Indra perception + Laya decisions) and its operator workspace.')}
<body class="home-page">${header()}<div class="shell shell-home">
<main id="content">
<section class="home-hero">
<p class="eyebrow"><span class="tiny-cross">+</span> ICR STUDIOS / GITHUB PAGES</p>
<h1>Documentation,<br><em>not marketing.</em></h1>
<p class="home-lede">Two surfaces, one product family. The <strong>SDK</strong> documents the NAYEN v1 engine —
Indra perception, Laya decisions, the executor seam and the WASM host loop.
The <strong>Studio</strong> documents the operator workspace shipped as the Windows package.</p>
<div class="home-actions">
<a class="button button-primary" href="sdk-architecture.html">Read the SDK docs <span aria-hidden="true">→</span></a>
<a class="button button-secondary" href="studio-guide.html">Open the Studio guide <span aria-hidden="true">→</span></a>
<a class="button button-secondary ext" href="${productSite}">Live product site <span aria-hidden="true">↗</span></a>
<a class="button button-secondary ext" href="${demoSite}">Live engine demo <span aria-hidden="true">↗</span></a>
</div>
<p class="status-note"><span class="status-dot" aria-hidden="true"></span>Manual-review prototype · no bundled OCR weights · no validated accuracy claim.
Missing models produce failure evidence, never invented text.</p>
</section>
<section class="home-search" aria-labelledby="home-search-title">
<label id="home-search-title" for="home-search">Search all documentation <span class="search-hint">titles, sections and text · works offline</span></label>
<input id="home-search" type="search" placeholder="e.g. executor, profile, unlock, export, provenance…" autocomplete="off">
<ol id="search-results" aria-live="polite"></ol>
<p id="search-empty" role="status" hidden>No matches. Try a shorter query.</p>
</section>
${areaSection('sdk')}
${areaSection('studio')}
${footer()}
</main></div></body></html>`;
}

/* --------------------------------------------------------------------- build */
await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });
for (const page of pages) {
  await writeFile(path.join(out, page.output), renderPage(page));
}
await writeFile(path.join(out, 'index.html'), home());

// Fail the build on broken anchors between published pages.
for (const { source, suffix, from } of localLinks) {
  if (!suffix.startsWith('#')) continue;
  const id = decodeURIComponent(suffix.slice(1));
  if (id && !pageHeadings.get(source)?.has(id)) {
    throw new Error(`Broken documentation anchor: ${from} → ${source}${suffix}`);
  }
}

// Offline search index (embedded as a script, no fetch/CSP connect-src needed).
const searchIndex = pages.map((page) => ({
  output: page.output,
  title: page.title,
  label: page.label,
  area: AREAS[page.area].name,
  heads: page.headings.filter(({ depth }) => depth === 2 || depth === 3)
    .map(({ text, id }) => [text, id]),
  text: page.markdown.replace(/```[\s\S]*?```/g, ' ').replace(/[#>*`|[\]()]/g, ' ')
    .replace(/\s+/g, ' ').toLowerCase().slice(0, 4000),
}));
await writeFile(path.join(out, 'index-data.js'),
  `window.__NAYEN_DOCS=${JSON.stringify(searchIndex)};\n`);

// Assets: tokens are copied from the single source of truth, never re-declared.
await cp(path.join(root, 'packages/theme/tokens.css'), path.join(out, 'tokens.css'));
await cp(path.join(root, 'docs/media'), path.join(out, 'media'), { recursive: true });
await cp(path.join(here, 'fonts'), path.join(out, 'fonts'), { recursive: true });
for (const asset of ['styles.css', 'app.js', 'nayen-mark.svg']) {
  await cp(path.join(here, asset), path.join(out, asset));
}
await writeFile(path.join(out, '.nojekyll'), '');
console.log(`Built NAYEN v1 docs: index + ${pages.length} pages (${REGISTRY.filter((p) => p.area === 'sdk').length} SDK, ${REGISTRY.filter((p) => p.area === 'studio').length} Studio) in ${out}`);
