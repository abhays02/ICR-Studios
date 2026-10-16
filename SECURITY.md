# Security boundaries

This is a local transcription/review prototype, **not** a certified banking
system, signature verifier, identity checker or regulatory-compliance solution.
Use synthetic/non-sensitive documents until your organization has completed its
own threat model, access review, legal/model/data-license review, deployment
hardening and restoration testing. Do not publish tokens, keys or customer scans
in an issue/PR/screenshot.

## What is implemented

- Mandatory random operator bearer authentication; constant-time token matching,
  owner-scoped documents/jobs/images/events/templates/results, separate admin
  checks, strict input types and safe structured errors.
- AES-256-GCM application-layer encryption for stored images and sensitive JSON
  (OCR candidates/provenance/reviews/history/settings). Fresh random nonces,
  versioned envelopes, key IDs and owner/row/column/header-bound AAD prevent
  cross-row/context substitution. Sensitive ORM/bulk plaintext writes fail closed.
  Tamper/wrong-key failures return a generic integrity error, not plaintext.
- Keyring in server environment only; mixed-key reads, active-key writes and
  bounded restartable rotation. Generated key/token file0600; state dir0700.
- Bound upload bytes, MIME magic, decoded pixels, frame/page count, PDF rendering,
  CPU/address-space/wall time, JSON/request sizes, decoder contention and queue.
  Metadata is stripped; no source URL/image fetching route.
- Local-only lazy OCR adapter, approved safetensors/local artifacts, no remote
  code, no download at startup/status, no external fallback. Worker execution
  uses an owned killable subprocess and fencing to reject stale commits.
- In-memory-only browser bearer input, authenticated blob image fetch and SSE,
  React text escaping, no localStorage/sessionStorage/cookie credentials,
  no frontend provider keys, no third-party CDN/analytics dependencies.
- No-store responses, no-referrer, nosniff, restrictive same-origin CSP and
  permissions policy; backend access logging disabled. CSV formula neutralization.
- Nonroot/read-only Docker default, explicit private mount/caches, dropped
  capabilities, no-new-privileges and optional (not automatic) GPU access.

## Encryption is not a full disk or database boundary

Application AEAD does **not** encrypt the SQLite container/schema/indexes as a
whole. IDs, owner identifiers, record kinds, row counts, timestamps, job states/
event kinds, revisions, foreign keys, payload lengths and access patterns can
remain visible. The private failure ledger has hashed runtime/crop fingerprints
and operational timestamps/statuses, not OCR text; these still reveal metadata.
Key file and database on the same compromised host are not two independent
security domains. Filesystem permission, full-disk/database-volume encryption,
backup encryption and external key-management controls are separate operator
requirements.

Encryption at rest does not protect process RAM, decrypted server responses,
browser/extension compromise, screenshots, explicit export downloads, swap,
core dumps, a stolen unlocked session, traffic/size/timing metadata or whole-host
root compromise. The application does not provide secure memory wiping. Normal
logical deletion does not guarantee forensic erase of SQLite WAL, SSD blocks,
snapshots, logs, caches, backups or downloaded files. Disable crash dumps, protect
swap, restrict browser extensions and control exported files according to policy.

## Network and authorization

Loopback is the only published default. If deployed beyond loopback, terminate
TLS at a trusted ingress and prevent direct API bypass. Apply rate/connection,
request-body, timeout and network limits; review the supplied nginx reference.
Do not mistake CORS for authentication or a complete network firewall.
All administrative settings/quarantine/retention endpoints need admin credentials.
Tokens are not scoped enterprise SSO/MFA credentials; revoke/rotate in the server
environment and restart all API processes. Admins do not gain other owners' data.
Principal owner IDs and associated metadata are still sensitive.

The CSP permits inline styles for SVG transforms, **not inline scripts/eval**.
Field/model text is never injected as HTML. The evidence drawer and JSON exports
are still sensitive data; safe text rendering does not make their content public.

## Provider/SSRF boundary — explicitly absent, not “security tested”

No OpenRouter/vLLM outbound adapter, remote endpoint form, arbitrary URL,
capability discovery or provider-key persistence/dispatch is shipped. Jobs reject
remote consent and unsupported geometry operations. Therefore local failure
cannot invoke billed/off-host recognition. This fail-closed absence must not be
advertised as a tested SSRF-safe remote integration.

A future provider implementation needs a separate review: fixed admin-configured
endpoint, explicit admin **and per-job** consent, server-only transient
credentials, public-address/metadata/reserved/private denial, connection-time DNS
pinning/TLS hostname validation, no environment proxies/redirects, bounded bodies/
timeouts, explicit narrowly allowed loopback vLLM opt-in, capability-filtered
model discovery/connection testing, strict structured-output validation and real
provenance. No arbitrary request endpoint or automatic off-host fallback is
acceptable. Self-reported model confidence is not calibrated confidence.

## Rotation, retention and incidents

1. Isolate network ingress and stop affected API/worker processes.
2. Revoke exposed operator tokens in server-held environment and restart all
   instances; do not publish their values in a ticket.
3. For suspected key compromise, introduce a fresh active key with old decrypt
   keys retained, restart all writers, reencrypt with `icr rotate`, verify records
   and arrange secure backup/key escrow retirement. Rotation does not undo prior
   plaintext exposure.
4. Preserve only necessary access-controlled forensic evidence, without printing
   raw customer data/tokens into ordinary logs. Determine filesystem/browser/
   export/backup exposure as well as database exposure.
5. Apply appropriate retention/deletion in all systems. App retention alone does
   not clean historical backups or downloaded exports.

Use your organization's private security contact/reporting process. If using a
repository reporting channel, send only a minimal non-sensitive reproduction;
never live banking images, bearer tokens, environment files, encryption keys or
plaintext database dumps. There is no promised SLA or public bug bounty.

## Verification limits

Dependency versions were advisory-checked and lockfiles are pinned; that is not
an OS/container penetration test or production compliance audit. Local browser
and CPU Docker workflows were actually exercised with synthetic fixtures.
No real handwriting accuracy, remote-provider security, TLS ingress execution,
CUDA/MPS inference, optional model-runtime compatibility or production restore
drill is claimed. See [STATUS.md](docs/STATUS.md) for evidence and remaining gates.
