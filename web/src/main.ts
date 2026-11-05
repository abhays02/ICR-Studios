/**
 * NAYEN v1 — page bootstrap.
 * Order matters: reveals/hero/pipeline are pure DOM and run immediately;
 * driver detection does network HEAD probes, so telemetry + demo init follow
 * it asynchronously (the demo's run button stays disabled until an image is
 * loaded anyway).
 */
import './style.css';
import { selectDriver, type DriverInfo } from './engine/driver';
import { initHero } from './hero';
import { initPipeline } from './pipeline';
import { initReveal } from './reveal';
import { initTelemetry } from './telemetry';
import { initDemo } from './demo';

async function detect(): Promise<DriverInfo> {
  try {
    return await selectDriver();
  } catch {
    // selectDriver is guarded end-to-end; this is a last-resort so the demo
    // still initialises (and stays honestly labelled) if detection explodes.
    const { SimulatedDriver } = await import('./engine/simulated-driver');
    return {
      driver: new SimulatedDriver(),
      label: 'simulated',
      detail: 'driver detection failed',
      wasmGluePresent: false,
      metaJson: null,
      modelsPresent: [],
      modelsMissing: [],
      engineVersion: null,
    };
  }
}

function boot(): void {
  initReveal();
  initHero();
  initPipeline();
  void detect()
    .then((info) => {
      initTelemetry(info);
      initDemo(info);
    })
    .catch(() => {
      // detect() already swallows everything selectDriver can throw; this is the
      // belt-and-braces so a bootstrap bug can never leave the page unrendered.
    });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', boot, { once: true });
} else {
  boot();
}
