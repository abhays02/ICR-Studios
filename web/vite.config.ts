import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { defineConfig, type Plugin } from 'vite';

const here = path.dirname(fileURLToPath(import.meta.url));

/** Serve .wasm / .onnx with correct MIME types in dev and preview. */
function mimeFixes(): Plugin {
  const fix = (url: string | undefined, setHeader: (k: string, v: string) => void) => {
    if (!url) return;
    const clean = url.split('?')[0] ?? '';
    if (clean.endsWith('.wasm')) setHeader('Content-Type', 'application/wasm');
    if (clean.endsWith('.onnx')) setHeader('Content-Type', 'application/octet-stream');
  };
  return {
    name: 'nayen-mime-fixes',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        fix(req.url, (k, v) => res.setHeader(k, v));
        next();
      });
    },
    configurePreviewServer(server) {
      server.middlewares.use((req, res, next) => {
        fix(req.url, (k, v) => res.setHeader(k, v));
        next();
      });
    },
  };
}

export default defineConfig({
  // Works from any host path (Vercel root, GitHub Pages subpath, file share).
  base: './',
  plugins: [mimeFixes()],
  // The ort chunk is imported dynamically, deep inside a guarded path, so Vite
  // would discover it mid-request and re-optimize — which invalidates the module
  // graph and reloads the page in a loop. Declaring it up front makes the dev
  // server bundle it at startup instead.
  optimizeDeps: {
    include: ['onnxruntime-web/wasm'],
  },
  server: {
    // The vendored contract and tokens live inside web/ (see
    // scripts/sync-vendor.mjs), so the dev server needs no extra fs access.
    fs: { allow: [here] },
  },
  build: {
    target: 'es2022',
    // onnxruntime-web is dynamically imported only when the WAS detects the
    // wasm glue + models; keep it out of the entry chunk.
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('onnxruntime-web')) return 'ort';
          return undefined;
        },
      },
    },
  },
  // No COOP/COEP: onnxruntime-web runs single-threaded (wasm backend),
  // so cross-origin isolation is not required and would break previews.
});
