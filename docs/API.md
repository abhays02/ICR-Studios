# Backend → Studio contract (v1)

Run from repository root, Python 3.11+: `uv sync --extra test`, `uv run icr
init-secrets`, `. var/secrets.env`, `uv run icr serve`. All secrets remain server
environment; obtain your operator bearer token from the private file locally.
`ICR_CONFIG=config/cpu.yaml` selects a profile; `icr --config ... serve` also works.
Do not expose the server profile without a TLS reverse proxy, request/rate limits
and restricted network access. Default bind is 127.0.0.1:8000.

All `/api/v1/*` require an Authorization header using the bearer scheme; tokens must stay in browser
memory only, never URL/localStorage/sessionStorage. Multi-principal environment:
`ICR_TOKENS` maps each >=32-character token to `{"owner":"alphanumeric","admin":true|false}`.
Even admins see only their own documents/jobs/custom templates. Health endpoints
are public and minimal. There is no cookie/CSRF flow. No remote provider
keys, remote endpoints, image URLs, or external fallback exist.

Responses use `Cache-Control:no-store`. Errors:
`{"code":"machine_code","message":"safe text","request_id":"uuid","details":[]}`.
422 details contain only field paths/types, not submitted values. Standard statuses:
401/403 auth, 404 owner-scoped missing, 409 revision/conflict/review, 413 limit,
415 media, 422 invalid, 429 queue/decoder busy, 503 encrypted integrity failure.
Authenticated OpenAPI: `GET /api/v1/openapi.json`; use it for exact request types.

## Templates and gallery

- `GET /templates` → `{"items":[Template,...]}` (all paths below prefix `/api/v1`).
- `POST /templates` JSON `Template` → 201 immutable version; duplicate ID/version
  → 409. Custom templates are owner-scoped; built-ins shared read-only.
- `GET /templates/{template_id}/versions/{version}` → Template.
- `GET /samples` → `{synthetic:true,is_real_handwriting:false,items:[{sample_id,
  form_kind,width_px,height_px,alignment_required}]}`.
- `GET /samples/{sample_id}/image` → full-resolution PNG.
- `POST /samples/{sample_id}/import` (empty body) → 201 DocumentSummary.
  No ground truth is read. Sixth sheared fixture returns 422: reviewed alignment
  is not implemented. First five demonstrate plumbing, not real handwriting.

Template: `{template_id,template_version,schema_version:"1.0.0",form_kind,
width_px,height_px,page:1,coordinate_space:"template_pixels",
bbox_convention:"half_open_xyxy",synthetic:false,fields:[FieldSpec]}`.
FieldSpec: `{field_id,label,type,required:false,page:1,roi:[x0,y0,x1,y1],
line_rois:[],cells:[],numeric:false,choices:[]}`. IDs alphanumeric/underscore,
max64; template version max32. Types: `text,multiline,enum,decimal,checkbox,date,
comb,ifsc,pan,amount_words,amount_figures`. Unknown properties reject. Max100
fields AND 100 inference crops. Every rectangle is integer, in-bounds, half-open,
**original upright image pixels**. Subcrops must fit field ROI. Multi-line ROIs
are read in array order; comb `cells` read in array order, concatenate without
separator, and are mutually exclusive with line_rois. Without subcrops, use ROI.
Only explicit `numeric:true` enables O/I/l/S/B → 0/1/1/5/8 normalization.
Built-in numeric comb labels set this flag. Names are never spell-corrected.
Comb fields without cell boundaries may use their provided single line ROI.
Checkboxes are always manual (nullable booleans), not TrOCR predictions.

## Upload, pixels, documents

- `POST /documents?template_id=...&template_version=...`: **RAW request bytes**,
  `Content-Type:image/png|image/jpeg|application/pdf`. Not multipart/FormData.
  In JS: `fetch(url,{method:"POST",headers:{Authorization,...,
  "Content-Type":file.type},body:file})`. Bounded streaming ≤10MiB /30s.
  MIME magic checked; one frame/page, ≤25MP, canonical RGB PNG, EXIF orientation
  applied once and all metadata stripped. PDF renderer runs in a spawned process
  with 20s wall, 15s CPU and 1.5GiB address-space bounds, checks page dimensions
  before rendering at configured DPI. Image decoding has the same process bounds.
  Only one decoder per API process; contention →429. Result raster must match
  template dimensions exactly. No guessed rescaling/alignment/perspective warp.
  →201 `{id,revision,width_px,height_px,template_id,template_version,
  review_state:"draft",synthetic}`.
- `GET /documents?offset=0&limit=50` → `{items:[DocumentSummary]}`, max100.
- `GET /documents/{id}` → `{id,revision,template,width_px,height_px,
  review_state,synthetic}`.
- `GET /documents/{id}/image` → full-resolution canonical PNG.
- `GET /documents/{id}/crops/{field_id}` → exact original-pixel PNG crop.
- `DELETE /documents/{id}` →204, removes encrypted image, jobs/events/reviews;
  fences any in-flight worker through deleted job. This is logical deletion,
  not guaranteed forensic erasure of WAL/filesystem/backups.

Use image-sized SVG viewBox overlays with object-fit:contain; do not submit CSS
pixels. To change layout, POST a **new template version** and upload/import with
that version; existing documents/jobs retain immutable snapshots. Arbitrary
orientation overrides, deskew, denoise and automatic tiling/detection unsupported;
ordered manual line/cell boxes are supported. Re-upload an explicitly corrected
scan with a reviewed template instead of assuming the sheared sample aligns.

## Jobs and events

- `POST /documents/{id}/jobs` JSON `{idempotency_key:"8-128 safe chars",
  alignment_confirmed:true,remote_consent:false,rotation:0,deskew:false,denoise:false}`
  →202 `{id,state:"queued"}`. Only first two keys required. Reject unsupported
  operations, true remote consent and unknown keys. Key is owner-scoped; identical
  request returns same job, different document/body →409. One active job/document,
  queue capped100, global single durable inference slot. Rerun using a **new key**.
- `GET /jobs/{id}` → `{id,document_id,state,fields,error,config_generation,
  progress:{done,total}}`.
- `POST /jobs/{id}/cancel` → `{id,state}`. Running owned subprocess is terminated
  and reaped; stale result commits are rejected with a monotonically raised fence.
- `GET /jobs/{id}/events?after=0`: genuine durable SSE, `event:progress`,
  `id:<monotonic seq>`, data `{job_id,kind,state}`. Heartbeat comments; terminal
  stream closes, nonterminal stream reconnect after60s. Resume by Last-Event-ID
  or after (not auth query parameters). Use authenticated **fetch streaming**, not
  EventSource, which cannot set Authorization. Poll GET job on progress; no field
  text appears in event rows. Existing terminal history is replayable.

States: `queued,running,needs_review,partial,failed,cancelled`; never automatically
approved. `completed` reserved terminal enum but not emitted by this review-only
pipeline. Checkpoints are encrypted per-field; hard timeout leaves honest partial
evidence, no invented values. Process death leaves a running lease; on reclaim it
is fenced and marked failed, not silently replayed. Lease ceiling1830s. One
attempt per crop (no auto-retry); persistent quarantine skips repeated failures,
including missing weights before any import/load. Admin reset required for same
fingerprint; dependency/config/artifact changes yield a new fingerprint.

`fields` keyed by ID: `{field_id,raw_text,normalized_value,confidence:null,
confidence_kind:"unavailable",status,issues,bbox,line_rois,model_used,provenance}`.
Provenance contains runtime fingerprint fields (artifact/config/lock/library,
device/dtype/attention/quantization), owner-keyed crop HMAC, ordered actual
attempt outcomes, seconds and `human_review_required:true`. Failed multiline
candidates retain `partial_raw_lines`, not a misleading complete raw_text.
No self-reported/calibrated confidence, no fake inference fallback.

## Manual corrections, review, export

- `GET /documents/{id}/results` → `{schema_version,document_id,form_kind,
  template_id,template_version,revision,review_state,job_id,job_state,fields,
  car_lar_match:null|bool,amount_reconciliation:{status,match},warnings}`.
  Each field extends job evidence with `value`, `validation_issues`, `review`
  (null or `{value,reviewer,at,revision}`). Missing inference yields raw null;
  manual review works without weights or a job.
- `PATCH /documents/{id}/fields/{field_id}` JSON `{revision,value}` → full results;
  CAS applies to **document revision**, not field revision. String/bool/null only,
  max4096 characters. Text requires string/null; checkbox bool/null. Stale →409.
  Corrections and append-only history are encrypted; original model raw text is
  never overwritten. Correcting/rerunning invalidates final review. Rerunning
  retains prior values but marks reviews `stale:true`; explicitly re-save each
  field to confirm it against new evidence before final approval.
- `POST /documents/{id}/review` JSON `{revision}` → full results. Requires explicit
  human corrections/review of every field, nonempty required values, no validation
  issues, no active job and no amount mismatch/unparseable pair. Optional blank
  strings/nulls allowed. Calendar dates accept ISO or DD/MM/YYYY; ambiguous
  slash dates remain flagged; use reviewed ISO to resolve. IFSC/PAN are format
  only, not bank/identity verification. Fixture schema enums are loaded into choices.
- `GET /documents/{id}/export?format=json|csv&mode=draft|reviewed`. Draft is
  explicitly unverified; reviewed blocked until final review. JSON preserves
  raw/candidate/review/provenance; CSV values neutralize spreadsheet formula
  prefixes. No plaintext server export/result cache.

**Amount semantics:** CAR=Courtesy Amount Recognition (amount **words**),
LAR=Legal Amount Recognition (amount **figures**). Generic `amount_words` and
`amount_figures` IDs/types reconcile with Decimal; supports Indian lakh/crore,
Indian/Western figure grouping, hundredths/paise; distinguishes unknown vs
unparseable vs mismatch. Earlier CAR/LAR synthetic *form abbreviations* are
unrelated; never infer amount field roles from form kind. Unsupported language/
grammar is reviewable/unparseable, not guessed.

## Settings, models and administration

- `GET /settings` and `GET /settings/export` (admin) →
  `{revision,values,sources,runtime_keys,secrets:"[REDACTED]"}`. No credentials
  are part of the schema. Only safe hot keys can be reimported.
- `GET /settings/schema` (admin) → `{schema,runtime_keys,help}`.
- `PATCH /settings` or `POST /settings/import` JSON
  `{revision,values:{max_tokens:128,attempt_seconds:60,job_seconds:300}}`
  (subset allowed) → `{revision,applies_to:"new_jobs"}`. Atomic persisted CAS,
  audited; invalid/restart-only keys reject. Environment/explicit startup
  overrides cannot be weakened. Export/import intentionally asymmetric:
  use exported revision plus **only runtime_keys** values; never blindly replay
  restart-only/security values. Full YAML profile imports require offline file
  review/restart, not a remote settings mutation.
- `GET /models` → `{local_only:true,items:[{model_id,status,inference_tested:false}],
  providers:{openrouter:"blocked_unsupported",vllm:"blocked_unsupported"},
  external_fallback:false}`. Microsoft is explicitly a different identity than
  Riksarkivet. Exact other IDs stay unsupported/unverified. No weight downloads
  at startup/status calls; artifact-present is **not** loaded/tested readiness.
- `POST /admin/ledger/reset` (admin) → `{status:"reset_audited"}`; audit uses actor
  digest/time, no exception strings. `logs/issues_ledger.json` under private
  data_dir uses flock/fsync/atomic replace and process-shared fingerprints.
- `POST /admin/retention` (admin) → `{status:"retention_applied"}`. Automatic at
  startup/hourly too. Retention default7d, demo1d.
- `GET /healthz` → `{status:"alive"}`.
- `GET /readyz` → `{api:"ready",inference:"model_unavailable"|...,
  human_review_available:true,worker:"enabled"|"disabled"}`. API availability is
  independent from inference availability; no framework import or weight hashing.

No outbound provider adapter was shipped: therefore no origin/URL entry,
capability discovery, provider connection-test, provider-key persistence, consent
endpoint or SSRF-capable dispatch. This is a fail-closed absence, **not a tested
network security adapter**. Exact requested forensic VLM prompt is retained as a
literal in `ocr.py` for a future separately reviewed adapter, not sent by TrOCR.
