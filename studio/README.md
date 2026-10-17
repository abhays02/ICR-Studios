# ICR Studio

React19/TypeScript7/Vite8, scaffolded with `create-vite@9.2.1` and exact pins plus
npm lockfile. No CDN fonts, browser-persisted tokens or provider keys.

```sh
npm ci --ignore-scripts
npm run lint
npm test
npm run build
```

The production bundle is served same-origin by FastAPI; use the repository
`scripts/start-local`. Development `npm run dev` proxies API/health to8000.
`npm run test:smoke` exercises a real running stack with Chromium; see the root
README for the private smoke-secret environment and artifact paths.

## Separate public demo

The cloud-consented landing page lives in `demo/` and `src/demo/`, not the
offline application's entry point. `npm run dev:demo` opens
`http://127.0.0.1:5174/demo/index.html` and proxies only `/demo-api` to the
separate gateway on port 8010. `npm run build:demo` writes `dist-demo/`;
`npm run build` still writes only the offline Studio to `dist/`.
Tailwind is enabled only in the demo build. Neither entry uses CDN assets.

The landing illustration is synthetic and its example output is not an OCR
prediction. Live uploads require explicit cloud consent and a configured Space.
See the root README for the gateway configuration and model availability limits.

See [Studio guide](../docs/STUDIO.md), [API](../docs/API.md),
[configuration](../docs/CONFIGURATION.md) and [actual verification](../docs/STATUS.md).
