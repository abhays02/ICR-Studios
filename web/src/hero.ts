/**
 * Hero 3D parallax — pointer tilt + scroll drift on the CSS 3D stage.
 * Pure transform writes in one rAF loop; disabled (and the scene flattened)
 * for reduced motion, coarse pointers, small viewports and low-power devices.
 */

const FLAT_BELOW = 960;

function lowPower(): boolean {
  const nav = navigator as Navigator & { deviceMemory?: number };
  if (typeof nav.deviceMemory === 'number' && nav.deviceMemory < 4) return true;
  if (typeof nav.hardwareConcurrency === 'number' && nav.hardwareConcurrency <= 2) return true;
  return false;
}

export function initHero(): void {
  const hero = document.querySelector<HTMLElement>('.hero');
  const scene = document.getElementById('scene');
  const stage = document.getElementById('scene-stage');
  if (!hero || !scene || !stage) return;

  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const coarse = matchMedia('(pointer: coarse)');

  const flatten = (): void => {
    scene.classList.add('scene--flat');
    stage.classList.remove('scene__stage--live');
    stage.style.removeProperty('--rx');
    stage.style.removeProperty('--ry');
    stage.style.removeProperty('--dz');
  };

  if (reduced.matches || coarse.matches || lowPower() || innerWidth < FLAT_BELOW) {
    flatten();
    // Re-check when the user changes the OS setting or rotates/resizes.
    reduced.addEventListener('change', () => {
      if (reduced.matches) flatten();
      else initHero();
    });
    return;
  }

  // Base tilt comes from the design tokens, not hardcoded degrees.
  const styles = getComputedStyle(document.documentElement);
  const baseX = parseFloat(styles.getPropertyValue('--tilt-x')) || 14;
  const baseY = parseFloat(styles.getPropertyValue('--tilt-y')) || -22;

  stage.classList.add('scene__stage--live');

  let pointerX = 0;
  let pointerY = 0;
  let scrollX = 0;
  let targetZ = 0;
  let curX = 0;
  let curY = 0;
  let curZ = 0;
  let raf = 0;
  let inView = true;

  const apply = (): void => {
    stage.style.setProperty('--rx', `${(baseX + curX).toFixed(3)}deg`);
    stage.style.setProperty('--ry', `${(baseY + curY).toFixed(3)}deg`);
    stage.style.setProperty('--dz', `${curZ.toFixed(2)}px`);
  };

  const tick = (): void => {
    curX += (pointerX + scrollX - curX) * 0.07;
    curY += (pointerY - curY) * 0.07;
    curZ += (targetZ - curZ) * 0.07;
    apply();
    if (inView) raf = requestAnimationFrame(tick);
    else raf = 0;
  };

  const ensureLoop = (): void => {
    if (!raf && inView) raf = requestAnimationFrame(tick);
  };

  hero.addEventListener('pointermove', (event) => {
    const rect = hero.getBoundingClientRect();
    const nx = (event.clientX - rect.left) / rect.width - 0.5; // -0.5..0.5
    const ny = (event.clientY - rect.top) / rect.height - 0.5;
    pointerY = nx * 16; // rotateY follows horizontal pointer
    pointerX = -ny * 10; // rotateX follows vertical pointer
    ensureLoop();
  });

  hero.addEventListener('pointerleave', () => {
    pointerX = 0;
    pointerY = 0;
    ensureLoop();
  });

  // Scroll drift: the object recedes and levels out as the hero leaves.
  const onScroll = (): void => {
    const p = Math.min(Math.max(scrollY / Math.max(hero.offsetHeight, 1), 0), 1);
    targetZ = -p * 140;
    scrollX = p * 8; // gentle nose-up as it recedes
    ensureLoop();
  };
  addEventListener('scroll', onScroll, { passive: true });

  new IntersectionObserver(
    (entries) => {
      inView = entries.some((entry) => entry.isIntersecting);
      ensureLoop();
    },
    { threshold: 0 },
  ).observe(hero);

  apply();
}
