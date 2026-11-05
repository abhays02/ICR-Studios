/* NAYEN v1 docs — behaviour: theme toggle, offline search, nav filter,
   TOC scroll-spy, opt-in recording playback. No network calls. */
'use strict';

/* ------------------------------------------------------------- theme toggle
   The inline bootstrap in <head> already set data-theme from
   localStorage > prefers-color-scheme > light. The toggle persists changes. */
const themeToggle = document.querySelector('#theme-toggle');
if (themeToggle) {
  const applyToggleState = () => {
    const dark = document.documentElement.getAttribute('data-theme') === 'dark';
    themeToggle.setAttribute('aria-pressed', String(dark));
    themeToggle.querySelector('.toggle-label').textContent = dark ? 'Light' : 'Dark';
    themeToggle.title = dark ? 'Switch to the light reading theme' : 'Switch to the dark theme';
  };
  themeToggle.addEventListener('click', () => {
    const dark = document.documentElement.getAttribute('data-theme') === 'dark';
    const next = dark ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', next);
    try { localStorage.setItem('nayen-docs-theme', next); } catch { /* storage blocked */ }
    applyToggleState();
  });
  applyToggleState();
}

/* ---------------------------------------------------------- sidebar filter */
const filter = document.querySelector('#nav-filter');
if (filter) {
  const links = [...document.querySelectorAll('.side-nav a')];
  const hint = document.querySelector('#nav-filter-hint');
  const noMatch = document.querySelector('.no-match');
  const groups = [...document.querySelectorAll('.nav-group')];
  filter.addEventListener('input', () => {
    const query = filter.value.trim().toLowerCase();
    let visible = 0;
    for (const link of links) {
      const match = !query || link.dataset.search.includes(query);
      link.closest('li').hidden = !match;
      if (match) visible += 1;
    }
    for (const group of groups) {
      group.hidden = ![...group.querySelectorAll('li')].some((li) => !li.hidden);
    }
    noMatch.hidden = visible > 0;
    hint.textContent = query ? `${visible} matching page${visible === 1 ? '' : 's'}` : `${links.length} pages · offline`;
  });
}

/* ------------------------------------------------------------- home search
   Runs over the generated index embedded in index-data.js — fully offline. */
const searchInput = document.querySelector('#home-search');
if (searchInput) {
  const results = document.querySelector('#search-results');
  const empty = document.querySelector('#search-empty');
  const index = window.__NAYEN_DOCS ?? [];
  const clean = (value) => value.replace(/[<>&"']/g, '');
  searchInput.addEventListener('input', () => {
    const query = searchInput.value.trim().toLowerCase();
    results.replaceChildren();
    if (!query) { empty.hidden = true; return; }
    const hits = [];
    for (const page of index) {
      let score = 0;
      let where = page.title;
      if (page.title.toLowerCase().includes(query)) score += 6;
      for (const [text, id] of page.heads) {
        if (text.toLowerCase().includes(query)) { score += 3; where = text; break; }
      }
      if (!score && page.text.includes(query)) {
        score += 1;
        const at = page.text.indexOf(query);
        where = `…${page.text.slice(Math.max(0, at - 40), at + 80)}…`;
      }
      if (score) hits.push({ page, score, where });
    }
    hits.sort((a, b) => b.score - a.score);
    empty.hidden = hits.length > 0;
    for (const { page, where } of hits.slice(0, 12)) {
      const li = document.createElement('li');
      const a = document.createElement('a');
      const headMatch = page.heads.find(([text]) => text.toLowerCase().includes(query));
      a.href = page.output + (headMatch ? `#${headMatch[1]}` : '');
      const area = document.createElement('span');
      area.className = 'r-area';
      area.textContent = page.area;
      const strong = document.createElement('strong');
      strong.textContent = clean(page.title);
      const small = document.createElement('small');
      small.textContent = clean(where);
      a.append(area, strong, small);
      li.append(a);
      results.append(li);
    }
  });
}

/* -------------------------------------------------------------- scroll-spy */
const spyLinks = [...document.querySelectorAll('.toc a[data-spy]')];
if (spyLinks.length && 'IntersectionObserver' in window) {
  const headings = spyLinks
    .map((link) => document.getElementById(link.dataset.spy))
    .filter(Boolean);
  const setActive = (id) => {
    for (const link of spyLinks) {
      if (link.dataset.spy === id) link.setAttribute('aria-current', 'true');
      else link.removeAttribute('aria-current');
    }
  };
  const visible = new Set();
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) visible.add(entry.target.id);
      else visible.delete(entry.target.id);
    }
    if (visible.size) {
      // The topmost visible heading wins; fall back to document order.
      const first = headings.find((heading) => visible.has(heading.id));
      if (first) setActive(first.id);
    }
  }, { rootMargin: '-80px 0px -60% 0px', threshold: 0 });
  for (const heading of headings) observer.observe(heading);
  // Keep the spy truthful on load with a deep link.
  if (location.hash) setActive(decodeURIComponent(location.hash.slice(1)));
}

/* ------------------------------------------------ recording opt-in playback
   Studio walkthrough GIFs stay still images until explicitly played. */
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
for (const button of document.querySelectorAll('.recording-toggle')) {
  const image = button.parentElement.querySelector('img');
  const poster = image.getAttribute('src');
  const stopRecording = () => {
    image.setAttribute('src', poster);
    button.setAttribute('aria-pressed', 'false');
    button.textContent = 'Play recorded Studio demo';
  };
  button.hidden = false;
  button.addEventListener('click', () => {
    if (button.getAttribute('aria-pressed') === 'true') stopRecording();
    else {
      image.setAttribute('src', image.dataset.recording);
      button.setAttribute('aria-pressed', 'true');
      button.textContent = 'Stop recorded Studio demo';
    }
  });
  reducedMotion.addEventListener('change', () => { if (reducedMotion.matches) stopRecording(); });
  document.addEventListener('visibilitychange', () => { if (document.hidden) stopRecording(); });
}
