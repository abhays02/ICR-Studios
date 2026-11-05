/**
 * Record the real demo, end to end, in a real browser.
 *
 * This is deliberately a recording and not a render: it opens the deployed (or
 * locally previewed) site, waits for the WASM engine to report itself live,
 * loads a shipped form, runs the engine and lets the page draw its own boxes,
 * timings and Laya fields. Nothing in the video is scripted output — if the
 * engine were absent the recording would show the honestly-labelled simulated
 * driver instead, and we would notice.
 *
 *   node scripts/record_demo.mjs                      # against 127.0.0.1:4173
 *   DEMO_URL=https://… node scripts/record_demo.mjs   # against a deployment
 *
 * Output: docs/media/nayen-demo.mp4 plus a poster frame. ffmpeg is required.
 */
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
// Playwright lives in studio's devDependencies; resolve it from there so this
// script needs no dependencies of its own.
const { chromium } = createRequire(path.join(root, 'studio', 'package.json'))('playwright');
const url = process.env.DEMO_URL ?? 'http://127.0.0.1:4173/';
const outDir = path.join(root, 'docs', 'media');
const video = path.join(outDir, 'nayen-demo.mp4');
const poster = path.join(outDir, 'nayen-demo-poster.png');

const WIDTH = 1440;
const HEIGHT = 900;

const hold = (page, ms) => page.waitForTimeout(ms);

async function main() {
  const recDir = mkdtempSync(path.join(tmpdir(), 'nayen-rec-'));
  const browser = await chromium.launch();
  const context = await browser.newContext({
    viewport: { width: WIDTH, height: HEIGHT },
    recordVideo: { dir: recDir, size: { width: WIDTH, height: HEIGHT } },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();

  console.log(`recording ${url}`);
  await page.goto(url, { waitUntil: 'load' });

  // The whole point: only record once the real engine is live.
  await page.waitForFunction(
    () => document.getElementById('driver-chip-text')?.textContent === 'driver: wasm',
    undefined,
    { timeout: 90_000 },
  );
  await hold(page, 2500); // let the hero settle on camera

  await page.locator('#demo').scrollIntoViewIfNeeded();
  await hold(page, 1200);

  await page.locator('.samples__btn').first().click();
  await page.waitForSelector('#run-btn:not([disabled])', { timeout: 30_000 });
  await hold(page, 1200); // the loaded page on camera before the run

  await page.locator('#run-btn').click();
  await page.waitForSelector('#demo-results:not([hidden])', { timeout: 180_000 });
  await hold(page, 1500); // boxes drawn over the scan

  await page.locator('#lines-body').scrollIntoViewIfNeeded();
  await hold(page, 1800); // the recognized lines

  await page.locator('#fields').scrollIntoViewIfNeeded();
  await hold(page, 2200); // Laya's fields and the CAR/LAR cross-check

  await page.screenshot({ path: poster });

  await context.close(); // finalizes the recording
  await browser.close();

  const webm = path.join(recDir, readdirSync(recDir).find((f) => f.endsWith('.webm')) ?? '');
  execFileSync(
    'ffmpeg',
    [
      '-y', '-i', webm,
      '-vf', `scale=${WIDTH}:-2,format=yuv420p`,
      '-c:v', 'libx264', '-preset', 'slow', '-crf', '23',
      '-movflags', '+faststart', '-an',
      video,
    ],
    { stdio: 'inherit' },
  );
  rmSync(recDir, { recursive: true, force: true });
  console.log(`wrote ${video} and ${poster}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
