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

**Linux / macOS quick start:** install Python 3.11+, Node 24.21.0 / npm 11+
and uv 0.13.1 through your approved package process
(`python -m pip install uv==0.13.1`), then run `./scripts/start-local` from
the repository root. Open **http://127.0.0.1:8000**. Unlock with only the
operator bearer token in the private `ICR_TOKENS` mapping in `var/secrets.env`,
not the encryption key or whole mapping. Import a fictional form from
**Synthetic gallery**. Stop with **Ctrl-C**; reload or **Lock workspace**
clears the browser's in-memory token.

`./scripts/start-local` installs from `uv.lock`/`package-lock.json`, builds then
serves the compiled app on the same origin. It does not invoke inference-extra
installs or model provisioning. Python3.11+ is declared; validation used3.12.3.
Node24.21.0 is pinned in CI/Docker and compatible with the Vite/native
TypeScript tooling. No global frontend packages are required.

For manual startup:

```sh
uv sync --frozen
(cd studio && npm ci --ignore-scripts && npm run build)
uv run --frozen icr init-secrets   # once only; refuses to overwrite
. ./var/secrets.env
uv run --frozen icr serve
```

FastAPI serves Studio, API, images and events on the same origin. For frontend
development, keep the backend running and use `cd studio && npm run dev`.
Vite proxies to port 8000; never copy credentials into frontend environment files.
See [contributor verification](../studio/README.md#verification) for existing
lint, build, test and browser-smoke commands, and the
[isolated showcase guide](../studio/README.md#separate-public-demo) for cloud
gateway instructions. The showcase is not deployed with the documentation site.

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

## Windows release build

Windows packaging is a separate release step, not part of the static site.
The Windows build workflow must compile the **offline** Studio, package its
Python service and synthetic resources, and smoke-test the resulting executable
on Windows before offering a download. A Linux build or passing source tests
do not establish Windows compatibility.

The intended download is a portable review prototype, **not** an installer,
signed production release, bundled model or validated OCR engine. Keep the
whole extracted application directory together; an executable alone may not
contain its supporting files. Use only synthetic data until you have reviewed
the Windows-specific protection and runtime limits below.

### Download and run

The **Windows manual-review preview** workflow builds on Windows x64 and uploads
only a smoke-tested ZIP plus its SHA-256 checksum. Preview artifacts expire after
14 days; published release assets are separate. A version tag must match
`pyproject.toml` exactly (`vX.Y.Z`). Tag builds, or manual runs on an existing
version tag with publishing enabled, create a **draft prerelease** for maintainer
review—not an automatically public production release. Manual non-publishing
branch builds produce only Actions artifacts.

1. Check the successful Windows workflow run and download its artifact, or use
   [Releases](https://github.com/abhays02/ICR-Studios/releases) after a maintainer
   publishes the reviewed draft. No successful Windows build is implied by a
   successful Linux/source build.
2. Compare the downloaded ZIP with the accompanying `.sha256` using PowerShell
   `Get-FileHash -Algorithm SHA256`. A checksum detects corruption; it is not a
   code signature. These executables are unsigned; follow your organization's
   trust policy rather than bypassing security warnings.
3. Extract the **whole** ZIP into a user-owned folder. Keep `ICR-Studio.exe`,
   `_internal/` and the bundle manifest together. Do not run as administrator.
4. In PowerShell, change to the extracted `ICR-Studio` directory and run:

```powershell
.\ICR-Studio.exe init-secrets
.\ICR-Studio.exe serve
```

Open **http://127.0.0.1:8000**. Initialization refuses to overwrite existing keys.
To unlock Studio, privately open
`%LOCALAPPDATA%\ICR-Studio\var\windows-secrets.json` and use **only the token
key inside the nested `ICR_TOKENS` JSON mapping**, not `ICR_KEYS`, the complete
mapping or the complete file. Nothing secret is printed by initialization.
The browser retains the operator token only in memory. Stop with **Ctrl-C**.
An alternate loopback port is available with `--port 8012 serve`.

### Private workspace and runtime limits

The launcher stores writable state outside application resources under
`%LOCALAPPDATA%\ICR-Studio`; settings retain the required relative `var/`
data directory. `--workspace C:\PrivateParent\ICR-Workspace` selects another
workspace whose parent already exists. The launcher requires a **fixed local
NTFS volume**, rejects network paths and reparse points, and applies private
Windows DACLs granting the current account and SYSTEM access. POSIX `chmod`
alone would not provide this boundary. Other processes running as your account,
administrators and backups remain outside the application's threat boundary.

The private `var/windows-secrets.json` stores encryption keys and operator
tokens; it is not a distributable asset. Protect and back up the keyring
separately from encrypted database/image backups. Losing keys loses data.
Replacing the extracted application does not require deleting the workspace.
Do not remove that workspace to “upgrade,” copy it into release ZIPs or put it
under a static server. Full storage protection, retention and export caveats
in this guide still apply.

The preview binds to loopback only and contains neither Torch/Transformers,
approved weights nor the cloud gateway. Resource ceilings and process isolation
are platform-specific: Windows keeps upload/pixel checks and a killable
20-second image-decoder deadline, but **does not have the POSIX decoder's hard
memory/CPU limits**. Windows testing is not a production sandbox audit.
Successful startup/manual review does not establish inference compatibility,
handwriting accuracy or equal POSIX/Windows hardening.

### Rebuild on Windows

Use Python 3.12.3 x64, uv 0.13.1 and Node 24.21.0 with npm 11+.
From the repository root in PowerShell:

```powershell
uv sync --frozen --extra test
uv pip install --no-deps -r requirements/build-windows.txt
Set-Location studio
npm ci --ignore-scripts
npm run build
Set-Location ..
uv run --frozen --no-sync python -m unittest discover -s tests -p "test_windows*.py" -v
uv run --frozen --no-sync python packaging/build_windows.py
uv run --frozen --no-sync python packaging/smoke_windows.py dist/ICR-Studio/ICR-Studio.exe
```

Build output is `dist/ICR-Studio/`. Distribute the entire folder, never just the
`.exe`; the workflow additionally tests an extracted ZIP before uploading it.
The build allowlists offline resources and inventories bundled files in
`bundle-manifest.json`. It does not snapshot the repository or private state.
Do not synchronize the environment between installing build tools and freezing:
the `--no-sync` commands deliberately retain the separately pinned build tools.
PyInstaller cannot cross-compile an authentic Windows executable on Linux.

Publishing a binary does not close the [model and accuracy gates](STATUS.md).
Never package `var/`, credentials, private documents, model caches, the cloud
gateway or `studio/dist-demo/` into a distributable. Downloaded exports are not
encrypted by the application.

## Containers

With Docker Engine + Compose v2, run `./scripts/start-docker` and open
**http://127.0.0.1:8000**. Use the operator token from private
`var/docker/secrets.env`. The default CPU image has no Torch, Transformers or
weights. Stop with `docker compose down`; private state is retained.

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

## Public documentation site

The landing and docs site is independent of Studio, FastAPI and private runtime storage.
The homepage introduces the product with an interactive CSS 3D illustration;
the guide library renders the complete repository README, security policy, all Markdown guides
under `docs/` (including future nested guides), fixture provenance and the Studio
development README. Existing Markdown remains the source of truth; do not edit
generated HTML. The public output contains only those guides, documentation
assets and the synthetic media already committed under `docs/media/`.
Review all new documentation/media for secrets and private data before publishing.

From the repository root, using Node 22+ and Python 3:

```sh
cd docs/site
npm ci --ignore-scripts
npm run build
npm run preview
```

Open **http://127.0.0.1:4174**. The build produces `docs/site/dist/`, rewrites
guide links and checks internal section anchors. No application credentials,
backend, model weights, CDN scripts, remote fonts or analytics are needed.
Mermaid fences remain readable diagram source; each guide also has a separate
interactive workflow. Known guides have curated steps in `docs/site/workflows.mjs`;
new guides automatically get section-based exploration steps until curated.

Playback starts **paused**. Keyboard-accessible controls support play/pause,
previous/next, direct step selection, reset and speed selection. Reduced-motion
preferences disable automatic workflow animation; manual steps remain usable.
The real GIF uses a still poster until explicitly played and can be stopped.
All guides and workflow steps remain readable without JavaScript. Title/filename
search filters navigation; it is not a full-text search engine.

### Publish separately from the application

The **Documentation site** workflow builds on documentation-related pull
requests/pushes. In repository **Settings → Pages**, select **GitHub Actions**
as the build source. Once enabled, documentation pushes to the default branch
(`main` or `master`) automatically publish the landing page and guides.
Pull requests only build; they never deploy. To republish manually, run the
workflow on the default branch with **Publish to GitHub Pages** enabled.
The `github-pages` environment can require approval. Only the generated static
artifact is deployed; the workflow never builds or deploys Studio or the API.

The default target is **https://abhays02.github.io/ICR-Studios/**. This is a
deployment target, not proof of a currently live site. Merge the changes, check
the workflow's deployment URL, then verify landing navigation, every guide and
media on the deployed site. Pages settings and DNS require repository/domain
owner access and cannot be provisioned by changing source files alone.

For a docs subdomain you control:

1. In **Settings → Pages → Custom domain**, enter your actual docs hostname.
   No domain has been assumed or provisioned by this change.
2. Follow GitHub’s DNS instructions: a subdomain CNAME normally points to
   `abhays02.github.io`, without a repository path. Verify domain ownership using
   GitHub’s recommended DNS verification before publishing.
3. Wait for DNS/certificate provisioning, enable **Enforce HTTPS**, then verify
   navigation, media and section links at the actual hostname.
4. Add the verified public URL to the repository website field and README only
   after deployment succeeds. Keep custom-domain configuration in Pages settings;
   the build intentionally does not ship a hardcoded `CNAME`.

Relative asset/page URLs support both a project Pages path and a subdomain root.
Alternatively upload **only `docs/site/dist/`** to a separate static HTTPS host
with directory indexes enabled and the correct HTML/CSS/JS/SVG/GIF MIME types.
Never serve the repository root, `var/`, environment files, keys or model caches.
Do not reuse the authenticated application origin or change its ingress for docs.
