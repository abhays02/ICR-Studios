/**
 * Vendor the two files `web/` consumes from outside its own tree, so the site
 * builds from a bare checkout of `web/` alone (which is exactly what a Vercel
 * build gets).
 *
 *   node scripts/sync-vendor.mjs          # refresh web/engine + web/packages
 *   node scripts/sync-vendor.mjs --check  # fail if the vendored copies drifted
 *
 * The copies are committed. In a full clone they are refreshed from the single
 * source of truth on every dev/build; `--check` is what CI uses to prove nobody
 * edited a copy by hand.
 */
import { copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const web = path.resolve(here, '..');
const repo = path.resolve(web, '..');

const PAIRS = [
  [path.join(repo, 'engine', 'src', 'types.ts'), path.join(web, 'engine', 'src', 'types.ts')],
  [
    path.join(repo, 'packages', 'theme', 'tokens.css'),
    path.join(web, 'packages', 'theme', 'tokens.css'),
  ],
];

const check = process.argv.includes('--check');
let failed = false;

for (const [source, target] of PAIRS) {
  if (!existsSync(source)) {
    // Standalone checkout: the committed copy is the source of truth here.
    if (!existsSync(target)) {
      console.error(`missing vendored file and no source to copy from: ${target}`);
      failed = true;
    }
    continue;
  }
  if (check) {
    if (!existsSync(target) || readFileSync(source).compare(readFileSync(target)) !== 0) {
      console.error(`drift: ${target} does not match ${source}`);
      failed = true;
    }
    continue;
  }
  mkdirSync(path.dirname(target), { recursive: true });
  copyFileSync(source, target);
}

if (failed) process.exit(1);
if (!check) console.log('vendored engine contract + design tokens into web/');
