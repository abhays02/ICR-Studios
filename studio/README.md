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

See [Studio guide](../docs/STUDIO.md), [API](../docs/API.md),
[configuration](../docs/CONFIGURATION.md) and [actual verification](../docs/STATUS.md).
