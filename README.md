# ICR-bank · local review Studio

An executable, dark, responsive React/TypeScript workspace with an authenticated
FastAPI backend, encrypted storage, immutable templates, durable local jobs,
streamed progress, human corrections and draft/reviewed exports.

**This is a working manual-review and missing-model vertical slice, not a
validated banking OCR product.** No weights, real-handwriting dataset, accuracy
scores or calibrated confidence are included. Only the local Microsoft TrOCR
adapter exists; the other requested models, OpenRouter and vLLM remain explicitly
unsupported. There is no external fallback. See the [actual gates](docs/STATUS.md).

## One-command local setup

Prerequisites: Linux/macOS, Python 3.11+ (tested 3.12.3), **uv 0.13.1**,
**Node 24.21.0 / npm 11+**. Install uv using your approved package process
(`python -m pip install uv==0.13.1`).

```sh
./scripts/start-local
```

This performs locked Python/npm installs, builds Studio, creates private
`var/secrets.env` once and starts **http://127.0.0.1:8000**. It never fetches model
weights. Copy only the operator bearer token from the private `ICR_TOKENS` mapping
into the masked unlock field. Do not paste the encryption key or the entire JSON
mapping. Tokens stay in memory, not browser storage; reload/Lock clears them.
Keep `var/` private and out of source control. Stop with Ctrl-C.

Without the helper:

```sh
uv sync --frozen
(cd studio && npm ci --ignore-scripts && npm run build)
uv run --frozen icr init-secrets   # once only; refuses to overwrite
. ./var/secrets.env
uv run --frozen icr serve
```

The compiled app is served by FastAPI at `/`; API/images/events are same-origin.
There is no need to enable CORS. For frontend development only, run
`cd studio && npm run dev`; Vite proxies API/health to port8000, without copying
credentials into frontend environment files.

## Docker, CPU default

Docker Engine + Compose v2 are required:

```sh
./scripts/start-docker
```

Studio is again **http://127.0.0.1:8000**. The container is nonroot UID10001,
read-only except private data and bounded runtime tmpfs. The helper initializes
`var/docker/secrets.env`, owns **only that dedicated state directory** as UID10001,
and starts the service. Read the private token locally with authorized filesystem
access; never change the directory/key permissions to world-readable.
`docker compose down` stops it without deleting the bind-mounted state.

The default image is a small **CPU manual-review/API image**, without Torch,
Transformers or weights. Opt in to the larger dependency image for CPU inference:

```sh
docker compose -f compose.yaml -f compose.inference.yaml up -d --build studio
```

For NVIDIA (driver + NVIDIA Container Toolkit required), provision approved
offline weights first, then explicitly enable the GPU service:

```sh
docker compose --profile gpu up -d --build gpu
# http://127.0.0.1:8001 ; separate var/gpu database inside the data mount
```

The optional inference image/GPU execution are **not tested here**. No container
downloads weights at startup. CPU and GPU use float32; no VRAM or performance
claim is made. See [configuration/deployment](docs/CONFIGURATION.md).

## Workflow

1. Open **Synthetic gallery**: six clearly labeled script-font fixtures, not real
   handwriting. Import the first five; the sixth is blocked pending alignment.
2. Inspect full-resolution pixels, zoom/pan/view rotation and the original-pixel
   SVG overlays. Edit labels/types/numeric flags and ordered line/comb-cell boxes.
   Undo/redo/reset; save a **new immutable template version**. Re-upload with it
   to apply changes; existing documents/jobs do not mutate.
3. Upload PNG/JPEG or a bounded single-page PDF (10MiB, exact template dimensions).
   Bulk queue progress measures real bytes, not guessed OCR progress.
4. Explicitly confirm upright alignment and queue local OCR. Authenticated fetch
   streams genuine SSE. Missing weights produce actual failure evidence/raw null.
5. Inspect real raw/model/provenance evidence and save each human correction.
   Final review is blocked for incomplete/invalid/stale fields or amount mismatch.
6. Download explicitly **draft** or **reviewed** JSON/CSV. JSON preserves raw
   evidence and human history; CSV neutralizes spreadsheet formula prefixes.

## Verification

```sh
(cd studio && npm ci --ignore-scripts && npm run lint && npm test && npm run build)
uv sync --frozen --extra test
uv run --frozen --extra test python -m unittest discover -s tests -v
docker compose config --quiet
docker build --target cpu -t icr-bank:studio .
```

Real browser smoke requires a running stack, private smoke environment and an
installed Chromium executable. It does not mock recognition or API responses:

```sh
# Defaults: URL http://127.0.0.1:8000, secret file var/studio-live/secrets.env
cd studio
CHROMIUM=/usr/bin/chromium npm run test:smoke
```

Override `ICR_SMOKE_URL` and repository-relative `ICR_SMOKE_SECRETS` as needed.
The smoke intentionally creates synthetic documents, template versions,
corrections and runtime setting changes; **never run it against production data**.
Screenshots/downloads go to ignored `var/`, never public static assets.

## Documentation

- [Verified implementation, execution evidence and open gates](docs/STATUS.md)
- [Studio/operator guide](docs/STUDIO.md)
- [Configuration and deployment](docs/CONFIGURATION.md), [all real keys](docs/CONFIG_REFERENCE.md)
- [Authenticated API / exact pixel and SSE contracts](docs/API.md)
- [Security boundaries and incident handling](SECURITY.md)
- [Evaluation methodology and absent measurements](docs/EVALUATION.md)
- [Upstream model/license evidence](docs/MODELS.md), [design](docs/DESIGN.md)
- [Fixture provenance](data/samples/README.md)

No signature/identity verification, automated approval, regulatory certification
or production banking compliance is claimed.
