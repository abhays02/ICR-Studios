import { cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Marked } from 'marked';
import { workflows } from './workflows.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const out = path.join(here, 'dist');
const repository = 'https://github.com/abhays02/ICR-bank/blob/main/';
const escape = (value) => String(value).replace(/[&<>"']/g, (char) => ({
  '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
}[char]));
const inlineText = (tokens) => tokens.map((token) => token.type === 'html' ? ''
  : token.tokens ? inlineText(token.tokens) : token.text ?? token.raw).join('');
const slug = (value) => value.toLowerCase()
  .replace(/[^\p{L}\p{N}_\s-]/gu, '').trim().replace(/\s/g, '-');

async function discover(directory) {
  const files = [];
  for (const entry of await readdir(path.join(root, directory), { withFileTypes: true })) {
    if (['site', 'media'].includes(entry.name)) continue;
    const name = path.posix.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await discover(name));
    else if (entry.name.endsWith('.md')) files.push(name);
  }
  return files.sort();
}

const sources = ['README.md', ...await discover('docs'), 'SECURITY.md',
  'data/samples/README.md', 'studio/README.md'];
const pages = await Promise.all(sources.map(async (source, index) => {
  const markdown = await readFile(path.join(root, source), 'utf8');
  return {
    source, markdown,
    output: index === 0 ? 'index.html' : `${source.replace(/\.md$/, '').replaceAll('/', '-')}.html`,
    title: markdown.match(/^#\s+(.+)$/m)?.[1] ?? source,
  };
}));
const bySource = new Map(pages.map((page) => [page.source, page]));
const localLinks = [];
const pageHeadings = new Map();

function resolveURL(href, page, image = false) {
  if (/^(https?:|mailto:)/i.test(href)) return image ? '' : href;
  if (/^[a-z][a-z\d+.-]*:|^[/\\]{2}/i.test(href)) return '';
  const match = href.match(/^([^?#]*)([?#].*)?$/);
  const pathname = decodeURIComponent(match[1]);
  const suffix = match[2] ?? '';
  const source = pathname ? path.posix.normalize(path.posix.join(path.posix.dirname(page.source), pathname)) : page.source;
  if (bySource.has(source)) {
    localLinks.push({ source, suffix, from: page.source });
    return bySource.get(source).output + suffix;
  }
  if (source.startsWith('docs/media/')) return `media/${path.posix.basename(source)}${suffix}`;
  if (source.startsWith('../') || source.startsWith('/') || source.includes('\\')) return '';
  return repository + source.split('/').map(encodeURIComponent).join('/') + suffix;
}

function render(page) {
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
        return `<h${depth} id="${escape(id)}">${text}<a class="anchor" href="#${escape(id)}" aria-label="Link to section">#</a></h${depth}>`;
      },
      link({ href, tokens, title }) {
        const url = resolveURL(href, page);
        return `<a href="${escape(url)}"${title ? ` title="${escape(title)}"` : ''}>${this.parser.parseInline(tokens)}</a>`;
      },
      image({ href, text }) {
        const url = resolveURL(href, page, true);
        if (url === 'media/studio-walkthrough.gif') {
          return `<figure class="recording"><img src="media/studio-review.png" data-recording="${escape(url)}" alt="${escape(text)}" loading="lazy"><button class="recording-toggle" type="button" aria-pressed="false" hidden>Play recorded Studio demo</button><figcaption>Real synthetic-only recording. Playback is optional; stop returns to a still image.</figcaption></figure>`;
        }
        return `<img src="${escape(url)}" alt="${escape(text)}" loading="lazy">`;
      },
    },
  });
  const content = markdown.parse(page.markdown);
  pageHeadings.set(page.source, new Set(headings.map(({ id }) => id)));
  const workflow = workflows[page.source] ?? {
    label: 'Explore this guide',
    steps: headings.filter(({ depth }) => depth === 2).slice(0, 4)
      .map(({ text }) => [text, 'Read the corresponding section below for the authoritative details.']),
  };
  if (!workflow.steps.length) workflow.steps.push(['Read', 'Explore the complete guide below.']);
  const nav = pages.map((item) => `<a href="${item.output}" data-search="${escape(`${item.title} ${item.source}`)}"${item === page ? ' aria-current="page"' : ''}>${escape(item.title)}</a>`).join('');
  const toc = headings.filter(({ depth }) => depth === 2).map(({ text, id }) =>
    `<a href="#${escape(id)}">${escape(text)}</a>`).join('');
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; object-src 'none'">
<meta name="description" content="${escape(page.title)} — ICR-bank documentation, evidence and interactive workflows.">
<title>${escape(page.title)} · ICR-bank docs</title><link rel="stylesheet" href="styles.css"><script src="app.js" defer></script></head>
<body><a class="skip" href="#content">Skip to documentation</a>
<header><a class="brand" href="index.html">ICR<span> / </span>DOCS</a><span class="badge">LOCAL FIRST · HUMAN REVIEW</span><a href="https://github.com/abhays02/ICR-bank">GitHub ↗</a></header>
<div class="layout"><aside class="sidebar"><label for="search">Find a guide</label><input id="search" type="search" placeholder="Title or filename…" autocomplete="off">
<nav aria-label="Documentation guides">${nav}</nav><p id="no-results" hidden>No matching guides.</p>
<p class="sidebar-note">Public documentation only.<br>No tokens. No private data. No API connection.</p></aside>
<main id="content"><section class="hero"><p class="eyebrow">THE EVIDENCE-LED WORKSPACE</p>
<h1>${escape(page.title)}</h1><p>Follow the workflow. Inspect the evidence. Keep the human in control.</p>
<p class="notice">Manual-review prototype · No validated OCR accuracy or production-readiness claim.</p></section>
<section class="workflow" aria-labelledby="workflow-title"><div class="section-heading"><div><p class="eyebrow">INTERACTIVE WORKFLOW · ILLUSTRATION, NOT LIVE EXECUTION</p>
<h2 id="workflow-title">${escape(workflow.label)}</h2></div><span id="step-count"></span></div>
<ol class="steps">${workflow.steps.map(([title, detail], index) => `<li><button class="step" type="button" data-step="${index}" aria-pressed="${index === 0}"><span class="step-number">${index + 1}</span><strong>${escape(title)}</strong><span>${escape(detail)}</span></button></li>`).join('')}</ol>
<div class="controls" hidden><button id="previous" type="button">← Previous</button><button id="play" type="button" aria-pressed="false">Play workflow</button><button id="next" type="button">Next →</button><button id="reset" type="button">Reset</button><label>Speed <select id="speed"><option value="5000">Slow</option><option value="3000" selected>Normal</option><option value="1500">Fast</option></select></label></div>
<p id="step-detail" role="status" aria-live="polite">All steps are available above. Playback is optional.</p>
<noscript><p>JavaScript is off. All workflow steps and the full documentation remain readable.</p></noscript></section>
<details class="toc"><summary>On this page</summary><nav aria-label="On this page">${toc}</nav></details>
<article>${content}</article><footer><a href="${repository}${page.source}">View Markdown source ↗</a><span>Built from repository docs · Synthetic public demos only</span></footer>
</main></div></body></html>`;
}

await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });
for (const page of pages) await writeFile(path.join(out, page.output), render(page));
for (const { source, suffix, from } of localLinks) {
  if (!suffix.startsWith('#')) continue;
  const id = decodeURIComponent(suffix.slice(1));
  if (id && !pageHeadings.get(source).has(id)) {
    throw new Error(`Broken documentation anchor: ${from} → ${source}${suffix}`);
  }
}
await cp(path.join(root, 'docs/media'), path.join(out, 'media'), { recursive: true });
for (const asset of ['styles.css', 'app.js']) await cp(path.join(here, asset), path.join(out, asset));
await writeFile(path.join(out, '.nojekyll'), '');
console.log(`Built ${pages.length} complete guides with interactive workflows in ${out}`);
