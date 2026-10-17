const search = document.querySelector('#search');
const guides = [...document.querySelectorAll('.sidebar nav a')];
search.addEventListener('input', () => {
  const query = search.value.trim().toLocaleLowerCase();
  for (const guide of guides) guide.hidden = !guide.dataset.search.toLocaleLowerCase().includes(query);
  document.querySelector('#no-results').hidden = guides.some((guide) => !guide.hidden);
});

const steps = [...document.querySelectorAll('.step')];
const play = document.querySelector('#play');
const speed = document.querySelector('#speed');
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
let current = 0;
let timer;

function select(index, announce = true) {
  current = (index + steps.length) % steps.length;
  steps.forEach((step, i) => step.setAttribute('aria-pressed', String(i === current)));
  document.querySelector('#step-count').textContent = `${current + 1} / ${steps.length}`;
  if (announce) document.querySelector('#step-detail').textContent = `Step ${current + 1}: ${steps[current].textContent.trim()}`;
}

function pause() {
  clearInterval(timer);
  timer = undefined;
  play.textContent = 'Play workflow';
  play.setAttribute('aria-pressed', 'false');
  document.querySelector('.workflow').classList.remove('playing');
}

function start() {
  pause();
  if (reducedMotion.matches) return;
  play.textContent = 'Pause workflow';
  play.setAttribute('aria-pressed', 'true');
  document.querySelector('.workflow').classList.add('playing');
  timer = setInterval(() => select(current + 1), Number(speed.value));
}

steps.forEach((step, index) => step.addEventListener('click', () => { pause(); select(index); }));
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
