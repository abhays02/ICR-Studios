/**
 * Scroll reveals.
 *
 * Preferred path: native CSS `animation-timeline: view()` (see the @supports
 * block in style.css) — no JS per-frame work at all. Fallback: one
 * IntersectionObserver adds `.is-visible`, which also drives the measurement
 * bars and the pipeline progress line. Reduced motion short-circuits both to
 * the final state.
 */
export function initReveal(): void {
  const targets = [...document.querySelectorAll<HTMLElement>('[data-reveal]')];
  if (!targets.length) return;

  const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const nativeTimelines =
    typeof CSS !== 'undefined' && CSS.supports?.('animation-timeline: view()') === true;

  if (reduced || nativeTimelines) {
    for (const target of targets) target.classList.add('is-visible');
    return;
  }

  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          entry.target.classList.add('is-visible');
          observer.unobserve(entry.target);
        }
      }
    },
    { threshold: 0.12, rootMargin: '0px 0px -4% 0px' },
  );
  for (const target of targets) observer.observe(target);
}
