# Backend execution evidence and open gates

2026-10-15, role BACKEND. **Functional manual-review/missing-weight vertical
slice implemented; real handwriting inference and optional remote models remain
unverified/blocked.** Frontend/deployment are the subsequent specialist's scope.
No ground truth, generated text, mock provider, or sample label is used as OCR.

## Implemented

FastAPI/SQLAlchemy SQLite schema v1; authenticated owner-scoped images/jobs/
events/templates/results; private runtime directory; AES-256-GCM encrypted
image and sensitive JSON rows; fresh nonces and table/owner/row/column AAD;
ORM bulk plaintext guard; mixed-key reads/active-key writes and bounded,
restartable rotation; immutable templates and OCR evidence, encrypted correction
history; explicit human final review; redacted settings/source/schema and
restricted CAS runtime settings; encrypted job checkpoints, process-safe single
queue slot/fencing, bounded killable OCR child processes, actual durable SSE;
flock/fsync/atomic failure quarantine and audited reset; byte/pixel/page/decode
limits, EXIF normalization, full-resolution original-pixel crops/line/cell ROIs;
calendar/IFSC/PAN/numeric-box/amount validation; draft/reviewed JSON and safe CSV;
retention at startup/hourly/admin trigger; lightweight health/readiness/model
statuses; exact pinned universal `uv.lock`, optional inference extra, six
validated profiles and generated full-key configuration reference.

**Only local Microsoft TrOCR is implemented**, with lazy local-only processor/
model classes, no remote code, safetensors only, float32 CPU/CUDA/MPS detection,
bounded greedy decoding, unavailable confidence, truncation review flags and
unload by owned process termination. This is not a claim of tested weights.
No BF16→FP16 memory reduction, calibration, automated checkbox/signature
verification, arbitrary transforms/deskew/denoise, detector, multilingual
accuracy, automated approval, or production banking compliance is claimed.

## Verification commands and results

All commands from repository root using Python **3.12.3**. No existing lint
configuration/runner was present, so no new linter was introduced.

1. `TMPDIR="$PWD/var/build" python -m pip install --quiet uv==0.13.1`
2. `TMPDIR="$PWD/var/build" uv lock`
3. `TMPDIR="$PWD/var/build" uv sync --extra test`
4. `uv run --extra test python -m unittest discover -s tests -p 'test_backend.py' -v`
   — initial **19/19 passed** (5.952s).
5. `uv run python -m unittest discover -s tests -v`
   — intermediate **24/24 passed** (8.080s), including all four DATA tests.
6. `uv run python -m compileall -q backend tests scripts/download_models scripts/benchmark`
   — passed.
7. `uv run icr config-reference` — generated `docs/CONFIG_REFERENCE.md`.
8. `uv run icr doctor` — CPU float32; Torch/Transformers not installed;
   Microsoft `model_unavailable`; CUDA/MPS false (not probed without Torch).

Final validation after all backend changes:

- `uv lock --check` —75 locked packages, up to date.
- `uv run python -m unittest discover -s tests -p 'test_backend.py' -v`
  — **22/22 passed**, 5.869s.
- `uv run python -m unittest discover -s tests -v`
  — **26/26 passed**, latest rerun8.255s (22 backend +4 DATA).
- `uv run python -m compileall -q backend tests scripts/download_models scripts/benchmark`
  and `git --no-pager diff --check` —passed.

One intermediate run exposed a newly added test's misplaced local variable
(`NameError`), not a suppressed production failure; it was corrected and both
targeted/full suites rerun successfully. Test doubles live
**only** in `tests/test_backend.py`; tests explicitly identify `TEST_DOUBLE`.
Real missing-weight jobs exercise the production spawn/ledger adapter path.
Tests include actual multiple processes for competing claims/failure-ledger
updates, fencing/reclaim, cancel/deadline, bulk-encryption guard, tamper/wrong
key/AAD, bounded restartable rotation with old-key removal, config precedence/
strictness, upload/MIME/EXIF/PDF/pixels/crops, owner/admin/consent rejection,
settings redaction/CAS, encrypted review, stale review/rerun, immutable versions,
CSV safety and retention.

Advisory checks: every direct dependency version (including optional Torch/
Transformers and uv) was checked **before** adding. All 75 locked package
versions (74 excluding this project, including platform alternatives) were
subsequently checked; no advisories reported. PyPI versions and the lock were
resolved by uv, not handwritten package hashes. The build backend is pinned too.
Starlette1.8 emits a **test-only deprecation warning** for its httpx compatibility
path; tests pass; no runtime warning workaround or unnecessary tool was added.

### Actual HTTP smoke (not ASGI test double)

Started the CLI on `127.0.0.1:8765` with freshly generated private environment
credentials under ignored `var/backend-smoke-live`, using:

```sh
export ICR_DATA_DIR=var/backend-smoke-live ICR_PORT=8765
uv run icr init-secrets
. var/backend-smoke-live/secrets.env
uv run icr serve
```

A local urllib client read the server-held environment token without printing
it and performed health/readiness → authenticated sample01 import → queued job
→ actual background worker missing-weight failure → manual full_name correction
→ SSE replay → draft JSON export → deletion. Observed:

- health `alive`, API `ready`, worker `enabled`, inference `model_unavailable`;
- job terminal `failed`, **9/9 fields processed**, all `raw_text:null`;
- actual missing-weight failure and persistent skip, no fabricated text;
- correction `reviewed`, original `raw_text:null` preserved;
- **12 genuine durable progress events** replayed;
- draft export and HTTP204 deletion passed.

Owned server was stopped after the check; generated secret/state directory was
removed. No smoke token/credentials enter source or this document.

### Real model/network evidence

An actual bounded request to
`https://huggingface.co/api/models/microsoft/trocr-base-handwritten` again raised
`URLError`. No immutable revision/license was established, no model was
downloaded, and no real recognition/accuracy/latency was measured.
`scripts/download_models` only supports that **different Microsoft identity**;
requires explicit `--download --approve-license <reviewed-license-id>`, resolves
and re-fetches immutable40-hex metadata, inventories SHA256s/license/card/approval,
downloads safe data formats only with size/time limits and publishes manifest
last. `--verify` is offline and checks every inventoried file. It refuses pickle
weights, remote code, incomplete/unapproved artifacts and ID substitution.
Provisioning was **not end-to-end tested against real HF artifacts**. Interrupted
download parts are deleted by the owning attempt; completed partial artifacts
may be retried under the process lock. An interrupted manifest publication needs
operator inspection/removal of its private `.manifest.part` before retry.

`scripts/benchmark --manifest <evaluation-only-line-labels.json>` performs real
bounded local inference, CER/WER/exact match and per-sample cold/warm latency
with hardware/device evidence; failures emit `measurements:null`, not scores.
No benchmark results were fabricated. Labels are only evaluation references.

## Operator notes

See [API.md](API.md) for Studio's exact endpoint/schema/error/SSE contract;
[CONFIG_REFERENCE.md](CONFIG_REFERENCE.md) is generated from the actual settings.
`uv sync --extra inference` is explicit and potentially large (platform Torch
CUDA libraries); **not installed/tested here**. Never install optional packages
or fetch weights during app startup.

To rotate: retain existing decrypt keys, add a fresh random32-byte key to
`ICR_KEYS`, set `ICR_ACTIVE_KEY` to its ID, restart **all** writers, run
`uv run icr rotate` until zero remain (the command loops batches50). Active
writes use the new key; mixed-key reads continue. Back up SQLite consistently
(including WAL as needed), escrow keys separately, and retain old keys for old
backups. No plaintext backup/export cache is created. `icr migrate` creates or
validates schema1 and fails closed on unknown schema versions; this is not an
unimplemented automatic migration from future/third-party schemas.

Only one running inference job across API processes is admitted by SQLite
BEGIN IMMEDIATE. Local filesystem/single-host only, not NFS/multi-host.
Failure budget is **one actual attempt** per crop; deterministic failures
quarantine until fingerprint changes/admin reset. Expired leases fail and
fence rather than auto-replay. This deliberately omits automatic resource
downgrade, multi-model retries and potentially billed remote retry logic.

## Remaining explicit gates

- Exact requested HF candidates stay **unsupported/unverified**; no guessed
  interfaces/aliases/licensing. Riksarkivet is not claimed absent.
- OpenRouter/custom vLLM are **blocked unsupported**. No arbitrary endpoint
  accepts connections, no external fallback and no outbound inference exists.
  SSRF/connection pinning/provider discovery/structured output/consent dispatch
  cannot be claimed tested because no provider adapter is shipped. Future work
  must satisfy the full design's DNS/connection-time/TLS/no-proxy/no-redirect
  and informed-consent gates. Local failure cannot initiate external traffic.
- Real weights, current optional-library compatibility, safetensors availability,
  approved weight license, CUDA/MPS inference, real handwriting, bank-domain
  evaluation/calibration, performance/RAM budgets and restore drills on actual
  deployment remain unverified.
- Manual reviewed alignment must precede OCR. No automatic form detector,
  perspective correction or deskew is implemented; sheared sample import fails
  explicitly. PNG/JPEG and bounded **single-page** PDF are supported.
- Auth is local operator-token authorization, not enterprise identity. Server
  TLS/rate limiting must be supplied by deployment. Whole-host compromise,
  browser compromise, traffic metadata and physical remnants are out of scope.
- OCR processors do not run the retained VLM forensic prompt; it is kept
  literally for future reviewed VLM integration, never advertised as TrOCR.
