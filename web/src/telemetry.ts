/**
 * Telemetry strip — keeps the honest "what is actually loaded" readouts in
 * sync: nav chip, hero status strip, demo driver label, footer provenance.
 */
import { MODEL_KINDS, type DriverInfo } from './engine/driver';

function set(id: string, text: string, cls?: 'is-ok' | 'is-warn'): void {
  const node = document.getElementById(id);
  if (!node) return;
  node.textContent = text;
  node.classList.remove('is-ok', 'is-warn');
  if (cls) node.classList.add(cls);
}

export function initTelemetry(info: DriverInfo): void {
  const live = !info.driver.simulated;

  // Nav chip
  const chip = document.getElementById('driver-chip');
  if (chip) {
    chip.classList.toggle('chip--ok', live);
    chip.classList.toggle('chip--warn', !live);
    chip.title = `driver: ${info.label} — ${info.detail}`;
  }
  set('driver-chip-text', live ? `driver: ${info.label}` : 'driver: simulated');

  // Hero status strip
  set(
    'st-engine',
    info.engineVersion ? `loaded · ${info.engineVersion}` : 'not loaded',
    info.engineVersion ? 'is-ok' : 'is-warn',
  );
  set(
    'st-driver',
    live ? 'wasm — live inference' : 'simulated — fallback',
    live ? 'is-ok' : 'is-warn',
  );
  const allModels = info.modelsPresent.length === MODEL_KINDS.length;
  set(
    'st-models',
    `${info.modelsPresent.length}/${MODEL_KINDS.length} present`,
    allModels ? 'is-ok' : info.modelsPresent.length ? 'is-warn' : undefined,
  );

  // Demo driver label
  const demoLabel = document.getElementById('demo-driver-label');
  if (demoLabel) {
    demoLabel.classList.toggle('chip--ok', live);
    demoLabel.classList.toggle('chip--warn', !live);
    demoLabel.textContent = live ? `driver: ${info.label}` : 'driver: simulated — engine not loaded';
  }

  // Footer provenance
  set('footer-version', info.engineVersion ?? 'not loaded');
  set('footer-driver', `driver ${info.label}`);
}
