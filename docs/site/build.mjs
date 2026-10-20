import { cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Marked } from 'marked';
import { workflows } from './workflows.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const out = path.join(here, 'dist');
const project = 'https://github.com/abhays02/ICR-Studios';
const repository = `${project}/blob/main/`;
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
    output: index === 0 ? 'overview.html' : `${source.replace(/\.md$/, '').replaceAll('/', '-')}.html`,
    title: markdown.match(/^#\s+(.+)$/m)?.[1] ?? source,
  };
}));
const bySource = new Map(pages.map((page) => [page.source, page]));
const localLinks = [];
const pageHeadings = new Map();

function resolveURL(href, page, image = false) {
  if (/^(https?:|mailto:)/i.test(href)) return image ? '' : href.replace('github.com/abhays02/ICR-bank', 'github.com/abhays02/ICR-Studios');
  if (/^[a-z][a-z\d+.-]*:|^[/\\]{2}/i.test(href)) return '';
  const match = href.match(/^([^?#]*)([?#].*)?$/);
  const pathname = decodeURIComponent(match[1]);
  const suffix = match[2] ?? '';
  const source = pathname ? path.posix.normalize(pathname.startsWith('/') ? pathname.slice(1)
    : path.posix.join(path.posix.dirname(page.source), pathname)) : page.source;
  if (bySource.has(source)) {
    localLinks.push({ source, suffix, from: page.source });
    return bySource.get(source).output + suffix;
  }
  if (source.startsWith('docs/media/')) return `media/${path.posix.basename(source)}${suffix}`;
  if (source.startsWith('../') || source.startsWith('/') || source.includes('\\')) return '';
  return repository + source.split('/').map(encodeURIComponent).join('/') + suffix;
}

function head(title, description) {
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; object-src 'none'">
<meta name="description" content="${escape(description)}">
<meta name="theme-color" content="#0a0c0d"><title>${escape(title)} · ICR Studios</title>
<link rel="icon" type="image/svg+xml" href="media/icr-mark.svg"><link rel="stylesheet" href="styles.css"><script src="app.js" defer></script></head>`;
}

function header() {
  return `<a class="skip" href="#content">Skip to content</a><header class="site-header">
<a class="brand" href="index.html" aria-label="ICR Studios home"><span class="brand-mark" aria-hidden="true"></span>ICR<span class="brand-light">STUDIOS</span><span class="brand-docs">/ FIELD NOTES</span></a>
<nav aria-label="Main navigation"><a href="index.html#guides">Guides</a><a href="${project}/releases">Releases ↗</a><a class="github-link" href="${project}">GitHub ↗</a></nav></header>`;
}

function scene(workflow) {
  return `<figure class="scanner-scene" aria-labelledby="scene-caption">
<div class="scene-topline"><span><i class="status-dot"></i>DOCUMENT VIEWPORT</span><span>001 / SYNTHETIC STUDY</span></div>
<div class="scene-world" aria-hidden="true"><div class="viewfinder"></div><div class="scene-coordinate">X 0240<br>Y 0180<br><span>1:1 / SOURCE</span></div>
<div class="scanner"><div class="scanner-base"></div><div class="scanner-bed"></div><div class="scanner-rail rail-left"></div><div class="scanner-rail rail-right"></div>
<div class="paper paper-under"></div><div class="paper document"><div class="paper-top"><b>ICR</b><span>SYNTHETIC FORM<br>REVIEW COPY / 001</span></div>
<div class="paper-rule"></div><div class="paper-label">ACCOUNT HOLDER</div><div class="handwritten">Alex Morgan</div>
<div class="region region-name"></div><div class="paper-columns"><div><span>REFERENCE</span><b>DEMO—001</b></div><div><span>AMOUNT</span><b>1,250.00</b></div></div>
<div class="paper-lines"></div><div class="paper-label">REVIEW NOTE</div><div class="paper-note">Human verified</div><div class="region region-amount"></div>
<div class="paper-bottom">FICTIONAL DATA ONLY<span>01 / 01</span></div><div class="scan-beam"></div></div>
<div class="scanner-bridge"><span>ICR / OPTICAL REVIEW</span><i></i><i></i><i></i></div></div>
<div class="scene-tag tag-input"><span>01 / SOURCE</span><b>The original. Always.</b><small>FICTIONAL FORM / 001</small></div>
<div class="scene-tag tag-evidence"><span>02 / EVIDENCE</span><b>Inspect the region</b><small>PIXELS BEFORE PREDICTIONS</small></div>
<div class="scene-tag tag-correction"><span>03 / HUMAN REVIEW</span><b>Correction saved</b><i>✓</i></div>
<div class="scene-tag tag-export"><span>04 / OUTPUT</span><b>{ } JSON / CSV</b><small>TRACEABLE BY DESIGN</small></div>
<div class="scene-scale">−<span></span>+<b>ORIGINAL → EVIDENCE → DECISION</b></div>
</div><figcaption id="scene-caption"><span class="scene-stage">${escape(workflow.steps[0][0])}</span><span>Interactive illustration / not live OCR</span></figcaption></figure>`;
}

function workflowPanel(workflow) {
  return `<section class="workflow" data-stage="0" aria-labelledby="workflow-title"><div class="section-heading"><div><p class="eyebrow">WORKSPACE STUDY / 01</p>
<h2 id="workflow-title">${escape(workflow.label)}</h2></div><span id="step-count">1 / ${workflow.steps.length}</span></div>
<div class="workflow-body">${scene(workflow)}<ol class="steps">${workflow.steps.map(([title, detail], index) =>
    `<li><button class="step" type="button" data-step="${index}" aria-pressed="${index === 0}"><span class="step-number">0${index + 1}</span><strong>${escape(title)}</strong><span>${escape(detail)}</span></button></li>`).join('')}</ol></div>
<div class="controls" hidden><button id="previous" type="button" aria-label="Previous workflow step">←</button><button id="play" type="button" aria-pressed="false"><span aria-hidden="true">▷</span> Play workflow</button><button id="next" type="button" aria-label="Next workflow step">→</button><button id="reset" type="button">Reset</button><label>Speed <select id="speed"><option value="5000">Slow</option><option value="3000" selected>Normal</option><option value="1500">Fast</option></select></label></div>
<p id="step-detail" role="status" aria-live="polite">Select a stage to explore. Motion is paused; this illustration does not run inference.</p>
<noscript><p>All stages are described above. JavaScript is off; the complete guides and links still work.</p></noscript></section>`;
}

function directory(page) {
  return `<div class="guide-search"><label for="search">Find your guide</label><input id="search" type="search" placeholder="Search titles or filenames…" autocomplete="off"><span class="search-hint">${pages.length} complete guides</span></div>
<nav class="guide-links" aria-label="Documentation guides">${pages.map((item) =>
    `<a href="${item.output}" data-search="${escape(`${item.title} ${item.source}`)}"${item === page ? ' aria-current="page"' : ''}><span>${escape(item.title)}</span><small>${escape(item.source)}</small></a>`).join('')}</nav><p id="no-results" role="status" hidden>No matching guides. Try another title or filename.</p>`;
}

function footer(page) {
  return `<footer><a class="brand" href="index.html">ICR<span class="brand-light">STUDIOS</span></a><p>Evidence first. Human authority, always.</p><a href="${page ? repository + page.source : project}">${page ? 'View Markdown source' : 'Explore the repository'} ↗</a><span>Public docs · Synthetic illustrations only</span></footer>`;
}

function landing() {
  const link = (source) => {
    const [file, fragment] = source.split('#');
    if (fragment && !pageHeadings.get(file)?.has(fragment)) {
      throw new Error(`Broken landing anchor: ${source}`);
    }
    return `${bySource.get(file)?.output ?? 'overview.html'}${fragment ? `#${fragment}` : ''}`;
  };
  const roles = [
    ['01', 'For operators', 'From first scan to final review.', 'Import a fictional fixture, inspect its regions and make an explicit human correction.', 'docs/STUDIO.md', 'Open the operator guide'],
    ['02', 'For local builders', 'Your workspace. Your machine.', 'Follow local setup and verification, explore Windows packaging and keep model provisioning explicit.', 'docs/CONFIGURATION.md#source-runtime', 'Start locally'],
    ['03', 'For integrators', 'Follow the contract, not a guess.', 'Explore authentication, document jobs, progress events and export boundaries.', 'docs/API.md', 'Read the API guide'],
    ['04', 'For the curious', 'A separate public showcase.', 'Understand the isolated web demo and its explicit consent boundary before trying live mode.', 'studio/README.md#separate-public-demo', 'Explore the showcase'],
    ['05', 'For evaluators', 'Evidence before accuracy claims.', 'Separate implemented review features from model availability, measured accuracy and open gates.', 'docs/STATUS.md', 'See implementation status'],
    ['06', 'For maintainers', 'Understand what stays private.', 'Trace local storage, the worker and the human review boundary through the architecture.', 'docs/DESIGN.md', 'Explore the architecture'],
  ];
  return `${head('Document intelligence. Human judgment.', 'Explore ICR Studios: local document review, explicit human corrections and traceable exports. Complete guides and an interactive CSS 3D workflow.')}
<body class="landing">${header()}<main id="content">
<section class="landing-hero" aria-labelledby="landing-title"><div class="landing-intro"><div class="hero-title-block"><p class="eyebrow"><span class="tiny-cross">+</span> DOCUMENT INTELLIGENCE. HUMAN AUTHORITY.</p>
<h1 id="landing-title">Every line.<br><em>In your hands.</em></h1></div><div class="hero-context"><p class="hero-kicker">THE SIGNAL IS IN THE DETAILS.</p>
<p class="hero-lede">The original scan. The supporting evidence.<br>The judgment only you can make.</p><p class="hero-description">A local-first document workspace.<br>Inspect, correct and export. On your terms.</p>
<div class="hero-actions"><a class="button button-primary" href="${link('docs/CONFIGURATION.md#source-runtime')}">Start locally <span>↗</span></a><a class="button button-secondary" href="#guides">Explore the guides <span>↓</span></a></div>
<p class="prototype-note"><span class="status-dot"></span> MANUAL-REVIEW PROTOTYPE<br><span>No bundled weights or validated OCR accuracy.<br>Illustration, not a live inference result.</span></p>
 </div><div class="intro-index"><span>01 — IMPORT</span><span>02 — INSPECT</span><span>03 — CORRECT</span><span>04 — EXPORT</span><span class="index-end">SCROLL TO EXPLORE ↓</span></div></div>
${workflowPanel(workflows['README.md'])}</section>
<section class="principles" aria-label="Project boundaries"><div><span>01 / LOCAL STUDIO</span><p>Your documents stay in the local workflow.<br>No remote inference fallback.</p></div><div><span>02 / EXPLICIT REVIEW</span><p>Model evidence and human corrections<br>remain separate and traceable.</p></div><div><span>03 / HONEST BOUNDARIES</span><p>A working review flow, not a claim<br>of production-ready recognition.</p></div></section>
<section class="guide-section" id="guides" aria-labelledby="guides-title"><div class="editorial-heading"><div><p class="eyebrow">FIND YOUR WAY IN</p><h2 id="guides-title">Less searching.<br>More understanding.</h2></div><p>Choose a starting point.<br>Every guide is here, in full.</p></div>
<div class="role-grid">${roles.map(([number, role, title, detail, source, cta]) =>
    `<a class="role-card" href="${link(source)}"><div class="role-meta"><span>${number} / ${role}</span><span aria-hidden="true">↗</span></div><h3>${title}</h3><p>${detail}</p><span class="card-cta">${cta} <span aria-hidden="true">→</span></span></a>`).join('')}</div></section>
<section class="all-guides" aria-labelledby="all-guides-title"><div class="editorial-heading"><div><p class="eyebrow">THE COMPLETE REFERENCE</p><h2 id="all-guides-title">Nothing hidden.<br>Everything connected.</h2></div><a href="overview.html">Read the project overview ↗</a></div>${directory()}</section>
<section class="closing-note"><span class="tiny-cross">+</span><h2>Start with evidence.<br>Keep the final say.</h2><a class="button button-primary" href="${project}/releases">Browse releases <span>↗</span></a><p>See published artifacts and their documented scope.<br>Availability is not a production-readiness claim.</p></section>
${footer()}</main></body></html>`;
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
        if (!url) return `<span class="image-alternative">${escape(text)}</span>`;
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
  const toc = headings.filter(({ depth }) => depth === 2).map(({ text, id }) =>
    `<a href="#${escape(id)}">${escape(text)}</a>`).join('');
  return `${head(page.title, `${page.title} — complete ICR Studios documentation, evidence and interactive workflows.`)}
<body class="guide-page">${header()}<div class="layout"><aside class="sidebar">${directory(page)}
<p class="sidebar-note">LOCAL FIRST / HUMAN REVIEW<br>Public documentation only.<br>No tokens. No API connection.</p></aside>
<main id="content"><section class="hero"><a class="back-link" href="index.html#guides">← All guides</a><p class="eyebrow">THE FIELD GUIDE / ${escape(page.source)}</p>
<h1>${escape(page.title)}</h1><p>Follow the workflow. Inspect the evidence. Keep the human in control.</p>
<p class="notice">Manual-review prototype · No validated OCR accuracy or production-readiness claim.</p></section>
${workflowPanel(workflow)}
<details class="toc"><summary>On this page <span aria-hidden="true">↓</span></summary><nav aria-label="On this page">${toc}</nav></details>
<article>${content}</article>${footer(page)}</main></div></body></html>`;
}

await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });
for (const page of pages) await writeFile(path.join(out, page.output), render(page));
await writeFile(path.join(out, 'index.html'), landing());
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
console.log(`Built landing page + ${pages.length} complete guides with 3D workflows in ${out}`);
