const search = document.querySelector('#search');
const guides = [...document.querySelectorAll('.guide-links a')];
search.addEventListener('input', () => {
  const query = search.value.trim().toLocaleLowerCase();
  for (const guide of guides) guide.hidden = !guide.dataset.search.toLocaleLowerCase().includes(query);
  document.querySelector('#no-results').hidden = guides.some((guide) => !guide.hidden);
});

const steps = [...document.querySelectorAll('.step')];
const play = document.querySelector('#play');
const speed = document.querySelector('#speed');
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
const workflow = document.querySelector('.workflow');
let current = 0;
let timer;

const revealTargets = [...document.querySelectorAll(
  '.landing .workflow, .principles, .editorial-heading, .role-card, .all-guides .guide-search, .closing-note',
)];
if ('IntersectionObserver' in window && !reducedMotion.matches) {
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add('is-visible');
      observer.unobserve(entry.target);
    }
  }, { threshold: 0.08 });
  for (const target of revealTargets) {
    target.classList.add('reveal-ready');
    observer.observe(target);
  }
  reducedMotion.addEventListener('change', () => {
    if (!reducedMotion.matches) return;
    for (const target of revealTargets) target.classList.add('is-visible');
    observer.disconnect();
  });
}

function select(index, announce = true) {
  current = (index + steps.length) % steps.length;
  steps.forEach((step, i) => step.setAttribute('aria-pressed', String(i === current)));
  workflow.dataset.stage = String(Math.min(current, 3));
  document.querySelector('.scene-stage').textContent = steps[current].querySelector('strong').textContent;
  document.querySelector('#step-count').textContent = `${current + 1} / ${steps.length}`;
  if (announce) {
    const title = steps[current].querySelector('strong').textContent;
    const detail = steps[current].querySelector('span:last-child').textContent;
    document.querySelector('#step-detail').textContent = `Step ${current + 1}: ${title}. ${detail}`;
  }
}

function pause() {
  clearInterval(timer);
  timer = undefined;
  play.textContent = 'Play workflow';
  play.setAttribute('aria-pressed', 'false');
  workflow.classList.remove('playing');
}

function start() {
  pause();
  if (reducedMotion.matches) return;
  if (current === steps.length - 1) select(0);
  play.textContent = 'Pause workflow';
  play.setAttribute('aria-pressed', 'true');
  workflow.classList.add('playing');
  timer = setInterval(() => {
    select(current + 1);
    if (current === steps.length - 1) pause();
  }, Number(speed.value));
}

steps.forEach((step, index) => step.addEventListener('click', () => { pause(); select(index); }));
steps.forEach((step, index) => step.addEventListener('keydown', (event) => {
  const offset = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 }[event.key];
  if (offset === undefined && event.key !== 'Home' && event.key !== 'End') return;
  event.preventDefault();
  pause();
  select(event.key === 'Home' ? 0 : event.key === 'End' ? steps.length - 1 : index + offset);
  steps[current].focus();
}));
document.querySelector('#previous').addEventListener('click', () => { pause(); select(current - 1); });
document.querySelector('#next').addEventListener('click', () => { pause(); select(current + 1); });
document.querySelector('#reset').addEventListener('click', () => { pause(); select(0); });
play.addEventListener('click', () => { if (timer) pause(); else start(); });
speed.addEventListener('change', () => { if (timer) start(); });
document.addEventListener('visibilitychange', () => { if (document.hidden) pause(); });

function applyMotionPreference() {
  pause();
  play.disabled = reducedMotion.matches;
  play.title = reducedMotion.matches ? 'Automatic playback disabled by your reduced-motion preference; use Next or select a step.' : '';
  if (reducedMotion.matches) document.querySelector('#step-detail').textContent =
    'Reduced motion is on. Select a stage or use Previous / Next; automatic playback is disabled.';
}
reducedMotion.addEventListener('change', applyMotionPreference);
document.querySelector('.controls').hidden = false;
applyMotionPreference();
select(0, false);

for (const button of document.querySelectorAll('.recording-toggle')) {
  const image = button.parentElement.querySelector('img');
  const poster = image.getAttribute('src');
  function stopRecording() {
    image.setAttribute('src', poster);
    button.setAttribute('aria-pressed', 'false');
    button.textContent = 'Play recorded Studio demo';
  }
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
