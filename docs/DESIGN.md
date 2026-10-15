# ICR-bank implementation design

Status: **THINKING/design completed, implementation not yet claimed**. Date: 2026-10-15 UTC. Repository initially contained README and .gitignore only. This document is the build contract for sequential DATA, BACKEND and STUDIO work; implementation deviations must be documented honestly. Model evidence and external contract limitations are in [MODELS.md](MODELS.md).

## 1. Scope and acceptance boundary

Build a self-hosted, local-first form transcription/review application, not an automated bank decision system. The first vertical slice supports versioned templates, image upload, explicit field ROIs, bounded local OCR, schema validation, encrypted persistence and human correction/export in a React TypeScript studio.

Terminology is a **working assumption** because no jurisdiction or authoritative form definitions were supplied: **CAR = Customer Account Registration**, **LAR = Loan Application Request**. Treat these as application form kinds, not regulatory standards. DATA must publish the exact schema and fixture mapping; do not imply bank approval or legal compliance. A bank-specific profile can rename/add fields with a new schema version.

Initial support: sanitized PNG/JPEG, one page per upload, explicit CAR/LAR selection, fixed template dimensions and user-confirmed alignment. Multi-page PDF rendering, automatic homography/detection, checkbox recognition, multilingual handwriting and extra model backends are follow-on capabilities, not fake buttons or untested adapters. Reject unsupported input with a useful error.

Acceptance without weights: uploading, positioning, cropping, validation, encrypted save, manual review and export work; attempting OCR returns an explicit `model_unavailable` state with manual review available. **Synthetic fixtures/ground truth must never impersonate OCR output.** Acceptance with provisioned approved weights adds actual Microsoft TrOCR single-line inference. No weights are downloaded by design or test startup.

## 2. System structure

```text
Browser React + TypeScript studio (no external fonts/analytics/CDNs)
       | same-origin authenticated /api/v1
FastAPI API -- typed services -- SQLAlchemy repository -- SQLite (single host)
       |                              | encrypted sensitive columns
       |                              + encrypted image/crop blobs
       + durable job/attempt ledger -- isolated OCR worker -- approved local snapshot
       |
       + optional policy-gated outbound adapter (off by default)
```

Use a separate worker process for inference so CPU/GPU work and hard timeouts do not block the ASGI loop. Initially one inference slot per device. SQLite supports local single-host multi-process use with WAL, foreign keys, busy timeout and short transactions; not multi-host/NFS. PostgreSQL and a dedicated queue are a later deployment profile, not an excuse for an unsafe in-memory queue.

Proposed ownership/layout (names may change coherently; do not create empty modules):

| Path | Responsibility |
| --- | --- |
| `backend/app/main.py`, `api/` | app lifecycle, versioned authenticated routes, request/error contracts |
| `backend/app/config.py`, `schemas/` | typed config and public DTOs, CAR/LAR/result validation |
| `backend/app/models.py`, `db.py`, `crypto.py` | ORM models, sessions/migrations, AEAD keyring |
| `backend/app/services/` | documents, templates, crop transforms, normalization, review/export |
| `backend/app/ocr/` | provider protocol, real TrOCR implementation, inference coordinator |
| `backend/app/worker.py`, `ledger.py` | durable claims, deadlines, retries, failure fingerprints |
| `backend/tests/` | targeted unit/integration/concurrency and adapter contract tests |
| `frontend/src/` | typed API client, templates/workspace/results/settings views, accessible dark theme |
| `data/templates/`, `data/samples/`, `data/schemas/` | versioned synthetic layouts, licensed/generated fixtures, ground truth, schema |
| `scripts/` | implemented provisioning/check/backup commands only |
| `var/` (ignored) | runtime DB, encrypted blobs, model cache/manifests; never committed |
| `docs/`, deployment files | verified runbook, threat limitations, container profile |

Use Python 3.11+ and Pydantic v2/FastAPI/SQLAlchemy 2 for application code, React/TypeScript with Vite for the studio. Final package versions must be chosen and locked by implementing roles after advisory checks; no dependencies were added in THINKING. Keep heavy inference dependencies optional and separate from the API install; do not import Torch during basic app startup.

## 3. Pixel/layout contract

Every ROI identifies a page and **coordinate space**. Never mix CSS coordinates, original image dimensions and normalized template coordinates.

- Decode bounded bytes; inspect actual format, EXIF-transpose once, flatten transparency on white, convert RGB, strip metadata. Record encoded-original dimensions and canonical upright `width_px`, `height_px`; display and crop the same canonical image. Reject decompression bombs before allocating unbounded pixels.
- Template version includes `form_kind`, `schema_version`, canonical `width_px/height_px`, immutable field IDs, field labels/types, `required`, and line ROIs. Store half-open integer pixel rectangles `[x0,y0,x1,y1)` in template space. Validate finite values, `0 <= x0 < x1 <= width` and analogous y bounds, positive minimum area, unique IDs and expected page.
- An image with a different size/aspect is **not** silently assumed to align. Require exact template raster size or a reviewed alignment operation, recording the image-to-template transform. Arbitrary phone perspective remains unsupported unless a real homography is implemented and reviewed.
- A transform is an explicit invertible 3×3 homogeneous matrix with version/source/destination dimensions. With alignment, map all ROI polygon corners through the inverse template transform to source canonical pixels. Perspective crops use a real warp; axis-aligned bounding boxes alone are insufficient. Reject singular/unstable transforms. Persist the exact transform used.
- For initial identity alignment, cropping is exact pixel slicing. Clamp only a separately recorded small padding to page bounds; invalid/zero boxes are errors, not accidental 1×1 crops. Model resizing is separate and must not change stored geometry.
- A browser using `object-fit:contain` has scale `s=min(viewW/imageW,viewH/imageH)` and offsets `ox=(viewW-imageW*s)/2`, `oy=(viewH-imageH*s)/2`. Image point is `((clientX-left-ox)/s,(clientY-top-oy)/s)`. Invert pan/zoom before this mapping. CSS pixels differ from canvas backing pixels (`devicePixelRatio`); apply DPR only to drawing, not twice to pointer coordinates. Use SVG with an image-sized `viewBox` where possible.
- Persist image-space geometry, not canvas screen pixels or percentages. Quantize rectangle starts with floor and ends with ceil only at the final crop. Geometry editing uses optimistic version checks; each job freezes template version, transform and crop digests.

Required transform tests: EXIF 90°/180° orientation, portrait/landscape, letterboxing, high-DPI display, zoom/pan, edge-clipped padding, resize roundtrip <=1 source pixel, rejection of NaN/inverted/outside boxes and misaligned dimensions.

## 4. Per-field pipeline and result truthfulness

1. Authenticate/authorize upload; apply byte/pixel/form/page limits. Generate server IDs, never trust filenames or paths. Store an encrypted canonical image and protected digest; freeze the selected template version.
2. Validate alignment and ROIs; present crops before recognition. For multi-line fields use explicit ordered per-line ROIs. Do not squeeze an entire address into a single-line model.
3. Create a durable job with frozen schema/template/config/model-artifact versions. Deduplicate only within the authorized owner and exact pipeline fingerprint.
4. Extract an immutable crop for each ROI. Optional contrast/deskew is deterministic, versioned and reversible in provenance; keep the original crop. Blank detection is a quality hint, not proof that a required field is absent.
5. Perform bounded local inference using the verified TrOCR processor/model interface. No user prompts, model URLs or Python code are executed. Failed/unsupported fields remain reviewable, not silently skipped.
6. Save raw recognized text and evidence. Normalize in a separate deterministic function with a list of transformations. Keep leading zeros, names' spelling and original punctuation where material; never change an identifier's O/0 or date order by guess.
7. Validate values and cross-field constraints. Warnings and errors annotate, never rewrite, evidence.
8. Optional explicit provider escalation can propose a **separate candidate**. It cannot overwrite raw local text or reviewed values. Human chooses changes; record the provenance and consent.
9. Save a field result/review history and job summary atomically with the fenced worker completion. Human edits require optimistic concurrency, rerun validation and preserve the original candidate.

Field result contract:

| Member | Meaning |
| --- | --- |
| `field_id`, `page`, `roi`, `line_rois` | stable field identity and canonical geometry |
| `raw_text`, `normalized_value` | nullable transcription and typed normalized candidate |
| `status` | `pending`, `recognized`, `blank`, `needs_review`, `unavailable`, `failed`, `reviewed` |
| `issues[]` | machine-readable rule ID, severity, field references, safe message |
| `confidence` | nullable structured evidence, never a made-up percentage |
| `provenance` | provider, model artifact/revision, processor/adapter version, config/template/schema version, crop digest, attempt ID, timings |
| `review` | reviewer identity, timestamp, corrected value, revision; separate from OCR evidence |

`confidence` distinguishes `kind:token_score|calibrated_probability|unavailable`, nullable `value`, aggregation/method, token count and calibration version. For TrOCR, optionally obtain generation scores (`return_dict_in_generate=True`, `output_scores=True`), calculate generated non-padding/non-special-token transition log probabilities, and report a length-normalized score. Beam/sampling and EOS exclusion must be defined and tested. **A token-likelihood score is not probability that the bank field is correct** and may be unavailable. Validation success, agreement between models, a blank-detection heuristic, and an LLM's self-rating are not calibrated correctness.

No initial automatic approval threshold. All critical bank fields require human review; confidence is evidence display only. Calibration requires real held-out handwriting, field-level correctness labels and reliability/Brier/ECE analysis. Disaggregate by field and language; a document aggregate cannot conceal a failed required field.

## 5. CAR/LAR typed schema and validation

Publish JSON Schema with `additionalProperties:false` for submitted business objects; use stable machine IDs and explicit `schema_version`. Distinguish absent/null, intentionally blank, unrecognized and invalid. Required validation applies when requesting final review/export, not when merely saving an incomplete draft.

Shared envelope: `schema_version`, `form_kind:CAR|LAR`, `document_id`, `template_id`, `template_version`, `fields`, `validation_issues`, `review_state`, `created_at` (UTC), `revision`. Public DTOs must be explicit; never serialize arbitrary ORM entities or ciphertext/key material.

Proposed initial business fields, for DATA to instantiate consistently:

| Form | Stable fields | Validation |
| --- | --- | --- |
| CAR | `full_name`, `date_of_birth`, `address`, `phone`, `email`, `account_type`, optional `tax_identifier`, `declaration_date`, `signature_present` | Name Unicode length/control-character rules; ISO calendar dates; syntactic contact checks; explicit account enum; declared profile for tax IDs; signature presence is nullable manual evidence, not biometric verification. |
| LAR | `full_name`, `date_of_birth`, `address`, `phone`, `email`, `requested_amount`, `currency`, `tenure_months`, `annual_income`, `employment_type`, `purpose`, `declaration_date`, `signature_present` | Decimal amount strings, configured currency, positive bounded integer tenure, nonnegative income, configured employment enum, bounded free text; no eligibility/affordability decision. |

Rules:

- Money is a decimal string with explicit currency and profile scale, never binary float. No locale guessing for ambiguous thousands/decimal separators. Preserve raw text and flag ambiguity. A configured maximum is validation policy, not a lending offer.
- Dates normalize only when unambiguous under a selected form locale; invalid calendar dates, future DOB and inconsistent declaration dates are issues. Do not infer missing years or silently swap day/month. No blanket minimum age without jurisdiction policy.
- Phone numbers remain strings; international normalization requires an explicit region. Email checks are syntax only, no outbound deliverability probe.
- Identity/account/routing identifiers remain strings; only apply PAN/IFSC/IBAN or other checksum/format rules under an explicit jurisdiction profile. Syntax/checksum passing does not prove ownership or existence.
- Signature and checkbox fields default to `null/needs_review` in TrOCR mode. Do not set “signed” because a crop is nonwhite. No OCR candidate can authorize a transaction.
- Cross-field errors reference all relevant IDs. Required-field completion and human approval gate a finalized export. Draft JSON export is allowed only when clearly marked unverified.
- JSON exports preserve raw/normalized/reviewed values and provenance; optional flat CSV uses reviewed values and neutralizes spreadsheet formula prefixes (`=`, `+`, `-`, `@`, tab/CR) in string cells.

## 6. ORM-level AEAD and retention

Use SQLAlchemy types/service hooks that encrypt sensitive values on bind and decrypt only in authorized service reads. Encrypt raw/normalized/reviewed text, business payloads, provider responses and identifying filenames. Images/crops are encrypted blobs too; ORM encryption alone does not protect files. Do not persist PII in jobs, traces, failure strings, request logs or analytics.

Envelope: `version`, `algorithm:"AES-256-GCM"`, `key_id`, fresh **96-bit random nonce**, ciphertext+tag. Keys are 32 random bytes from an operator-managed keyring outside the database/repository, not a password or a generated committed default. Authenticate associated data containing installation/owner ID, table, column, immutable row UUID and envelope version. Generate row UUID before encryption; a basic TypeDecorator without row context must not pretend to bind to a row. Use a context-aware write/read service or row event approach and test bulk-write bypasses.

Never reuse a nonce with a key. Corrupt tag/unknown key is a typed integrity failure; fail closed and log only non-sensitive IDs. Randomized ciphertext prevents equality lookup. If a lookup is genuinely needed, use a separate domain-separated HMAC key and owner-scoped blind index with documented equality leakage, not deterministic encryption or a plain digest of low-entropy account numbers.

Key rotation: configure one active write key plus old decrypt-only keys; new writes use active ID. Rotate in short restartable batches using compare-and-swap on old envelope/version, preserving AAD, and checkpoint progress without plaintext. Keep mixed-key reads working. Retire a key only after proving no live rows/blobs require it and handling historical backups; do not delete the last decryptor for retained backups. Rotation and restore tests are mandatory. Configuration redaction must never return key bytes.

Backups contain encrypted DB/blobs plus a separately protected key escrow process; test restoring both. Use owner authorization on every object ID, restrictive file permissions, TLS when not loopback, explicit retention/deletion, no caching of sensitive HTTP responses, and no PII in browser localStorage. Deletion removes references/blobs under retention policy; SQLite WAL/backups/filesystem remnants mean physical secure erasure is not guaranteed. This design does not protect plaintext while in authorized process memory, a compromised host/admin, or a malicious browser extension. AEAD does not make the whole database or all metadata opaque.

## 7. Interprocess ledger, retries and fingerprints

The database is the source of truth; never use a module-global lock or JSON file as the job queue. Suggested tables: `documents`, `template_versions`, `jobs`, `field_results`, `attempts`, `model_artifacts`, `artifact_files`, `consents`, `review_events`, `settings_versions`, plus encrypted blobs/columns.

Jobs transition `queued -> running -> completed|needs_review|failed|cancelled`; retries transition through `retry_wait -> queued`. Document archival/review status is separate from job execution status. Attempts are append-only records containing stage/model/config/input fingerprints, claim token, start/finish/deadline, sanitized outcome and error code.

- Claim atomically in a short SQLite `BEGIN IMMEDIATE` transaction with a conditional state/lease update, increment fencing token and return the claimed job. No transaction spans inference/network I/O.
- `lease_owner`, `lease_until` and monotonically increasing `fence` protect against a worker crash/reclaim. Heartbeat renewals and completion writes must compare the exact fence and owner. A stale worker may finish computation but cannot overwrite a newer result.
- Reclaim expired jobs only when the prior worker is fenced; retries can duplicate computation but commits remain conditional. Do not claim exactly-once remote billing.
- Unique owner-scoped idempotency key + request-body digest prevents duplicate upload/job requests; a different body with the same key is conflict. Apply ownership checks before returning a deduplicated result.
- Failure fingerprint is SHA-256 of canonical **non-secret** structured data: stage, adapter version, model artifact/revision, device/runtime capability class, safe error class/code, dependency-lock digest and relevant config hash. Do not hash raw exception messages containing paths, prompts, keys or PII. Per-input failures additionally use a separately protected owner-scoped HMAC crop digest; do not let one unreadable field globally quarantine a model.
- Persist error count, next retry time and last sanitized cause. Missing weights, incompatible API, bad artifact hash, license refusal, invalid config and authentication failure are non-retryable until the relevant fingerprint changes. Missing user input requires correction, not repeated inference.
- Default bounds: maximum **3 total attempts per field/stage** including first attempt, exponential delay with jitter bounded at 30s, per-attempt wall deadline and overall job deadline. Budget is persisted across restarts and different worker processes. Network libraries' internal retries must be disabled or counted within this budget.
- CUDA OOM gets at most one explicit resource downgrade (smaller batch or CPU if supported and allowed) under the same budget; otherwise fail/review. No infinite “clear cache and retry” loop. Kill only the owned inference subprocess on hard timeout, fence it, then reap it.
- Remote connect failure before dispatch may be retried within budget; read timeout after possible dispatch is `outcome_unknown`, requiring explicit retry consent because billing/exfiltration may already have occurred. Provider 429/5xx handling is bounded policy, not indiscriminate replay.

For future explicit weight provisioning, apply the same process-safe principles: artifact key includes repository+full SHA+filename; a unique download claim and lease coordinates writers; per-owner part files live under the project runtime model directory, never a global temp folder. Verify expected size/digest, fsync and atomic replace only on the same filesystem; publish a verified manifest last. Do not mark ready from file existence, permit path traversal, execute downloaded code, or retry deterministic checksum failures forever. Recovery removes only stale files owned by expired ledger claims. HF cache locking alone is not the application failure ledger.

## 8. Local-first, optional providers and access policy

Default `local_only=true`, all remote adapters disabled, no telemetry and no automatic model downloads. A model candidate listed in documentation is not a supported adapter. Microsoft local snapshot is the only initial OCR implementation contract; other candidates are evidence-gated follow-on work.

Remote processing requires all of: administrator-enabled provider, server-side credentials, explicit model/capability choice, owner authorization, per-document/field informed consent and a retained consent record naming recipient/data category/policy version. Prefer sending only requested crop bytes; text-only correction still discloses PII. Show payload scope before consent, use synthetic data for tests, support revocation before dispatch, and explain already-sent data cannot be recalled. No fallback from local to external merely because local OCR failed.

SSRF controls:

- Initial upload API accepts bytes, **not** fetchable URLs. No remote file import.
- OpenRouter has fixed HTTPS origin and path; redirects disabled. Never accept an arbitrary base URL in a document request or infer URLs from model content.
- An optional custom/local inference server is administrator-only, separate from cloud provider consent. Allowlist exact scheme/hostname/port and optional pinned network ranges. A loopback/private endpoint may be intentionally allowed only in that explicit local profile; generic cloud profiles reject loopback, RFC1918, link-local, multicast, IPv6-mapped private addresses and cloud metadata addresses.
- Resolve/validate destinations on every connection, prevent DNS rebinding with connection-time address enforcement and correct TLS host verification; validate every redirect or simply disable redirects. If the HTTP stack cannot guarantee this, do not expose arbitrary endpoints. An egress firewall/proxy is additional defense.
- Disable ambient proxy/environment routing unless explicitly configured; credentials never go to a redirected/different host. Apply response-size/connect/read/total limits and redact error bodies.

Access: all `/api/v1` operations including list/detail/image/export/settings/model status require authentication; health endpoint exposes only liveness, readiness only safe component status. Initial single-operator bearer token is loaded from environment/secret file, constant-time compared, never shipped in frontend bundle or put in URLs. Studio token is memory-only (re-enter on reload) unless a secure HttpOnly session flow is implemented. Operator mode is not tenant isolation or enterprise identity. If multiple principals are added, enforce owner scoping at repository level and admin scopes separately.

Same-origin deployment and explicit development CORS origins only; no wildcard credentialed CORS. If cookies replace bearer auth, add SameSite/secure cookies and CSRF protection before enabling writes. Bind loopback by default; remote access requires TLS/reverse proxy, upload limits and rate limiting. A deployable container is not evidence of production banking compliance.

## 9. Typed configuration and lifecycle

One immutable validated settings snapshot per generation. Proposed precedence, lowest to highest: code defaults < selected config file < operator persisted **allowlisted hot overrides** < environment < explicit CLI flags. Secrets only from environment/secret files, never DB/UI. Unknown keys, invalid values and conflicting security settings fail validation; do not silently coerce `"false"` to true.

| Setting class | Examples | Behavior |
| --- | --- | --- |
| Restart-only | DB/storage paths, host/port, CORS/auth, keyring, worker process count, device, model snapshot/runtime, provider origin/allowlist, `local_only` | Validate at startup; report `restart_required`; no apparent hot update that does nothing. |
| Hot, non-secret | review display preferences, bounded retry/deadline defaults, validation profile defaults, provider enablement **within startup security policy** | Atomic validated persisted version; applies to new jobs only; environment/CLI overrides remain authoritative. |
| Versioned artifacts | templates, schema, normalization/calibration rules | Create immutable new versions; old jobs retain old versions. |
| Per-job | selected template, allowed preprocessing, consented provider/fields | Validate against global policy, freeze when enqueued; cannot weaken global restrictions. |

Return redacted effective config with source per key, generation and restart requirements. UI must show when an environment setting overrides a saved value. A PATCH/PUT uses `If-Match`/version CAS; update is all-or-nothing and audited. Workers receive persisted generation IDs; multi-process hot changes must not rely on an in-memory singleton. In-flight jobs retain their snapshot except immediate consent/security revocation, which is checked again before each external dispatch. Runtime model/device changes require draining/restarting the affected worker and a readiness check.

Suggested explicit safe bounds to implement centrally: upload 10MiB, image 25 megapixels, one page, <=100 field/line ROIs, bounded text lengths, one inference slot, 3 attempts, configurable finite job deadline. These are **design defaults**, not measured capacity. Include all behavior-changing defaults in job/config fingerprints.

## 10. API and studio contracts

Proposed `/api/v1` resources:

- `GET /templates`, `POST /templates`, `GET /templates/{id}/versions/{version}`; versioned layout writes validate all ROIs.
- `POST /documents` (multipart image + form/template selection), `GET /documents`, `GET /documents/{id}`, `GET /documents/{id}/image`, `DELETE /documents/{id}`.
- `POST /documents/{id}/jobs` (idempotency key, frozen template/config, no implicit remote permission); `GET /jobs/{id}`, `POST /jobs/{id}/cancel`.
- `GET /documents/{id}/results`, `PATCH /documents/{id}/fields/{field_id}` with revision; explicit final-review action; `GET /documents/{id}/export?format=json|csv&mode=draft|reviewed`.
- `POST /documents/{id}/consents`, `DELETE /documents/{id}/consents/{consent_id}` only if a remote adapter is implemented.
- `GET /models` reports evidence/capability/readiness states without downloading; implemented provisioning via administrator CLI first, not a generic executable-model API.
- `GET /settings`, versioned allowlisted `PATCH /settings`; `GET /healthz`, safe `GET /readyz`.

Do not implement endpoints with successful placeholder responses. It is preferable to omit a deferred endpoint and document it than to return fabricated model availability.

Errors use `{code,message,request_id,details}` with bounded safe field-level details and proper HTTP status: 401/403 auth, 404 owner-scoped missing object, 409 revision/idempotency conflict, 413 size limit, 415 unsupported media, 422 invalid form/layout, 503 unavailable required service. Per-field inference failure is also represented in durable results, not erased by a generic 500.

Studio: dark-mode responsive workspace with local-only/provider status banner, upload/form/template selector, image+SVG ROI editor, crop preview, run/cancel/progress, field evidence/validation table, manual correction and export. Required fields, unknown confidence, unavailable weights and draft/reviewed status must be visually distinct. Accessible labels, keyboard editing, visible focus and adequate contrast; confidence not color-only. Use generated/shared OpenAPI types, not duplicated unchecked string maps.

Poll durable job state with bounded backoff and cancellation on unmount initially; don't add WebSocket complexity for a demo. Preserve only nonsensitive UI preferences locally. Never render model HTML with raw `innerHTML`; show text or sanitize with a reviewed policy if rich content is later needed.

## 11. Verification and implementation order

### DATA (next)

Create original **clearly synthetic** CAR/LAR images, template definitions, schemas and separate ground truth; include source/license provenance, deterministic generator if practical, exact dimensions and ordered line ROIs. Printed/script-font synthetic images are not real handwritten bank accuracy evidence. Include blank/invalid/leading-zero/multiline cases. Do not add customer data or download model weights.

### BACKEND

1. Typed settings, request/result schemas, auth and bounded image upload/cropping.
2. ORM AEAD and encrypted blobs, versioned templates and safe persistence.
3. Real offline-only TrOCR adapter plus explicit unavailable behavior; test protocol with a clearly labeled deterministic **test-only** fake, never a runtime ground-truth adapter.
4. Durable process-safe job/attempt ledger, deadline/retry/fence semantics and provenance.
5. Review/validation/export API and integration tests. Optional providers only after consent/SSRF contract is implemented; otherwise report deferred.

### STUDIO

Implement API-wired React studio, accurate transforms, crop review, edits/export and meaningful settings/model states. Add local deployment/run instructions, ignored persistent volumes and secret setup, lockfiles and health checks. Verify both no-weights manual-review mode and backend test suite. Do not describe the whole product as complete if model provisioning/remote validation remains unperformed.

### Required tests and release evidence

- Schemas: CAR/LAR required/null/enum/extra fields, Unicode, decimal strings, leading zeros, ambiguous dates, negative/overflow values, CSV formula safety.
- Geometry: canonical-image/crop correspondence and frontend transform cases listed above.
- Crypto: ciphertext not plaintext in DB/blobs, unique nonces, wrong key/tamper/AAD failure, key rotation resume, unauthorized access denied, no leaking exports/logs.
- Concurrency: **two actual processes**, one job claim, stale lease reclaim, stale worker completion rejected, idempotency conflict, retry budget survives restart, fingerprint changes only when relevant inputs/runtime change.
- Failure: unavailable weights, corrupt snapshot, model exception/timeout, partial field failures, cancel, no implicit download/network call, invalid image and image bomb/oversize rejection.
- Remote (only if shipped): no dispatch without consent, remote policy revoked mid-job, malicious origins/redirect/private IP/DNS rebinding, credential redaction, empty/error/invalid/truncated provider results, unknown billing outcome.
- UI/build: existing chosen package runner's typecheck/build/tests; a browser smoke test of upload→layout→job/unavailable→manual edit→draft/reviewed export. Label test fixtures, do not present them as live model inference.

No test suite existed during THINKING; documentation consistency and evidence were reviewed without adding tools/dependencies. Later roles must record exact commands/results and distinguish mocked tests from real provisioned-weight inference.

## 12. Open gaps / nonclaims

All HF model revisions and current weight cards remain unverified because DNS failed; even Microsoft needs a provenance/license gate before weight provisioning. OvisOCR2 has no verified inference adapter here. Chandra/Surya licensing may be unsuitable for banking without an agreement. No handwritten bank benchmark, GPU test, calibration study, real OpenRouter call, production auth/tenant policy, jurisdictional schema approval, automated alignment or deployment security audit was performed. Initial synthetic samples and manual review demonstrate plumbing, not production recognition accuracy. These gaps must remain visible in README/UI and release notes until independently closed.
