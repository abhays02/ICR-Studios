# Configuration, runtime and deployment

## Exact supported schema and precedence

The authoritative source is `backend/app/config.py`; the generated
[CONFIG_REFERENCE.md](CONFIG_REFERENCE.md) lists every key, real default,
constraint and environment variable. Regenerate with
`uv run --frozen icr config-reference`. Unknown YAML/schema keys fail closed.

Precedence: defaults < selected YAML < permitted persisted runtime values <
environment < explicit startup settings. `ICR_CONFIG=config/cpu.yaml` or
`icr --config config/cpu.yaml serve` selects a profile. Settings are strict:
numbers/booleans/lists in environment are parsed as JSON (`true`, not `True`;
origins as a JSON array). Secrets are **not settings**.

| Profile | Intent / actual behavior |
| --- | --- |
| `config/cpu.yaml` | CPU float32, local-only; default |
| `config/cuda.yaml` | Explicit CUDA float32; requires optional runtime/approved weights/device |
| `config/mps.yaml` | Explicit Apple MPS float32; unverified here |
| `config/local.yaml` | Loopback/local-only |
| `config/server.yaml` | Server binding, local-only inference; must be behind protected TLS ingress |
| `config/demo.yaml` | Local synthetic demo, one-day retention; no fake OCR |

Only `max_tokens`, `attempt_seconds`, `job_seconds` are hot runtime keys.
Admin PATCH/import checks current revision, constraints and authoritative
startup overrides, persists atomically and audits actor/key names. New jobs
snapshot effective settings; active jobs do not change underneath the worker.
All other keys are visibly restart-only in Studio.

Defaults include max10MiB upload,25MP raster,one PDF page,150DPI,100 fields/
inference crops,128 output tokens,60s crop attempt,300s job and7d retention.
The API/decoder/queue also have hard ceilings. Limits are not a hardware
capacity/performance promise. Real high-resolution multipage banking pipelines
are outside this release.

## Keys, token principals and private state

```sh
uv run --frozen icr init-secrets
. ./var/secrets.env
```

This creates the selected private data directory (0700) and environment file
(0600) once using cryptographic randomness. It refuses overwrite.
`ICR_KEYS` is a JSON key-ID to base64-encoded32-byte mapping;
`ICR_ACTIVE_KEY` selects writes. `ICR_TOKENS` maps random >=32-character tokens
to `{"owner":"alphanumeric","admin":true|false}`. Neither keyring nor token map
appears in configuration/schema/export. `.env.example` contains no usable
credentials. Never replace the generated values with example strings.

Even admins see only their own documents/jobs/custom templates. Admin privileges
cover settings, retention and failure ledger operations. Token ownership is
local operator authorization, not enterprise SSO/MFA/identity proofing.
Change/revoke tokens by editing server-held environment and restarting **all**
API processes. No browser persistence or provider credentials exist.

Data directory must be relative beneath `var/`; symlink traversal is rejected.
Keep database, key file, failure ledger and caches off public/web-served paths.
Default SQLite storage is single-host/local filesystem; NFS and distributed
multi-host workers are unsupported. The source-mounted encrypted data directory
is not a static route.

## Source runtime

`./scripts/start-local` installs from `uv.lock`/`package-lock.json`, builds then
serves the compiled app on the same origin. It does not invoke inference-extra
installs or model provisioning. Python3.11+ is declared; validation used3.12.3.
Node24.21.0 is pinned in CI/Docker and compatible with the Vite/native
TypeScript tooling. No global frontend packages are required.

For explicit optional **local CPU inference**:

```sh
uv sync --frozen --extra inference
. ./var/secrets.env
uv run --frozen --extra inference icr doctor
uv run --frozen --extra inference icr serve
```

Keep `--extra inference` on uv run; otherwise uv may synchronize without that
extra. This optional install is large and includes platform Torch components.
It was **not** installed/tested here. `doctor` is diagnostic; loading artifacts
and successful real inference are separate evidence gates.

Provisioning is a separate reviewed operation: see
`scripts/download_models --help` and [MODELS.md](MODELS.md). It requires explicit
download and license approval, immutable revision/file inventory and offline
verification. Do not invent a license ID, substitute checkpoint namespaces, set
`trust_remote_code`, download weights during service startup or claim
artifact-present as inference-tested. HF metadata/network failure remains an
open gate; no model is bundled.

## Containers

The multi-stage Dockerfile builds Studio in pinned Node, copies actual backend
source, templates/schemas/gallery images and profile files, then installs the
project **editable at `/app`** from frozen `uv.lock`. This is deliberate:
backend resource roots resolve from the source tree, not an incorrectly relocated
site-packages directory. The final CPU image is nonroot UID/GID10001 and includes
compiled Studio only (not node_modules, keys, private files, `.git`, real data).

`./scripts/start-docker` uses a dedicated `var/docker` bind mount. The initialized
key file travels with private state for convenience in this local deployment,
but key escrow/backups must be separate in a serious deployment. The helper's
short root initialization sets ownership inside **that mount**, not the whole
repository or host. Application processes remain nonroot. The mount path can be
overridden by `ICR_VOLUME_DIR` in the operator environment.

Compose makes the root filesystem read-only; private state/caches are mounted
and runtime scratch is bounded tmpfs `/app/run`. All capabilities are dropped
and no-new-privileges is enabled. HF/Torch caches are under the private mount;
no secret/model directory is mounted into the static document root. Docker's
healthcheck probes liveness, not successful OCR.

- `docker compose up -d --build studio`: CPU review/API image; no optional OCR libraries.
- `docker compose -f compose.yaml -f compose.inference.yaml up -d --build studio`:
  opt-in dependencies, still CPU. Approved weights must already be provisioned.
- `docker compose --profile gpu up -d --build gpu`: opt-in NVIDIA device request,
  separate `var/gpu` DB, port8001, shared approved model/cache mount.
- `docker compose down`: stop without deleting bind state.

GPU needs a compatible host NVIDIA driver and Container Toolkit. A successful
Compose config/build is **not** a CUDA/MPS test. The default build was actually
built and run; optional inference/GPU image execution remains unverified.
Image tags pin versions, not immutable registry digests; operators should mirror,
scan and pin approved digests in controlled deployments. OS/container image CVE
scanning and production hardening are not claimed by dependency advisory checks.

## Network/TLS/rate-limit boundary

Local defaults publish only loopback. FastAPI serves the compiled SPA and API
same-origin, without cookies or permissive CORS. The dev Vite server is loopback
and proxies to loopback8000; it is not a production server. Do not expose it.

External deployment requires operator-managed HTTPS, trusted certificates,
authentication-safe ingress, request-size/time/concurrency/rate limits and network
policy. [deploy/nginx.conf](../deploy/nginx.conf) is a reference ingress with
TLS1.2+, IP rate/connection limits,10MiB body ceiling and SSE buffering disabled;
it must be installed in your reviewed nginx topology and was **not** exercised
as a TLS endpoint here. Install certificates privately, never in the repository.
Restrict direct API access so ingress cannot be bypassed. Tune rates for gallery
images/SSE without disabling abuse controls. Never log Authorization, full paths
with identifiers, bodies or OCR text. Backend access logs are disabled.

CORS defaults empty; if truly necessary, explicitly list trusted HTTPS origins
with no paths/wildcards. Loopback HTTP is development-only. Header-token auth does not
use cookies, so no cookie/CSRF flow is implemented. CSP restricts scripts/connect
to self, images to self/blob, denies frames/objects/base injection and only allows
inline **styles** for coordinate transforms. React escapes field/model text; it
is not rendered as HTML. No third-party CDN fonts/scripts/analytics.

## Liveness, readiness and missing artifacts

- `/healthz`: process alive.
- `/readyz`: API/human-review availability, worker enabled/disabled and actual
  lightweight artifact status. `model_unavailable` does **not** make manual
  review unavailable. Presence does not imply loaded/tested recognition.
- Authenticated `/api/v1/models`: supported adapter/artifact status and explicit
  unsupported providers/candidates; never downloads at status calls.
- Queued job evidence reports actual dependency/artifact/runtime failures,
  timing and unavailable raw values. Persistent failure quarantine prevents
  repeating the same failed attempt silently.

Do not make readiness silently download/import large model frameworks or report
fake successful OCR. There is no automatic fallback/off-host connection.

## Retention, rotation, backup and recovery

Retention runs at startup/hourly, and explicitly via admin endpoint. Default7d,
demo1d. It deletes application documents/jobs/events/reviews; no forensic erase
of WAL/filesystem/backups/browser downloads is promised.

For key rotation: add a fresh random key under a new ID **while retaining old
decrypt keys**, switch `ICR_ACTIVE_KEY`, restart all writers, then
`uv run --frozen icr rotate`. Bounded restartable batches reencrypt rows.
Verify completion and backups before removing any old key. Old backups still
need their old keys; losing keys loses data. `icr migrate` creates/checks schema1
and rejects unknown versions, not a magic migration from arbitrary databases.

Back up SQLite consistently (WAL/checkpoint or supported SQLite backup method);
store encrypted backups and keys in separate access domains. Restore into an
isolated network, validate schema/key/AAD integrity, principal mapping, retention,
sample crops/reviews and worker lease fences before exposing it. Restore drills
and disaster-recovery timings are **not measured in this release**.
