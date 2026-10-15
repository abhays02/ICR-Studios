# Studio operator guide

## Session boundary

Unlock with a >=32-character operator token from the private server environment,
not an encryption key. The masked input is cleared immediately on submission.
The token lives in React/closure memory only, is sent in an Authorization header,
and is never placed in URLs, cookies, localStorage or sessionStorage. Images use
authenticated fetch plus revocable in-memory blob URLs. Lock clears documents,
evidence and token state and aborts active requests/uploads. Reload requires
another unlock. Do not use a shared/untrusted browser, extensions or public
screen-sharing session for banking data. Browser memory and downloaded exports
are not encrypted by the application.

## Gallery and uploads

Every gallery item is **synthetic script-font text**, not real handwriting.
First-five import uses the backend's real encrypted-document endpoint, without
reading ground truth. The sheared sixth fixture is visibly blocked: reviewed
alignment is not implemented. It is not secretly rescaled/rotated/accepted.

Select a template before uploading. PNG/JPEG/single-page PDF are sent as raw
request bytes, with their actual MIME type, not multipart data. Backend enforces
magic, dimensions, byte/pixel/page limits and bounded decoding. EXIF orientation
is applied once, metadata stripped and RGB PNG stored. Documents must match the
template's original upright dimensions; resizing/alignment is never guessed.

Drag files onto the upload card or use the multi-file chooser. Uploads are
sequential because the backend admits one bounded decoder per process.
States are queued/uploading/server decoding/stored/failed/cancelled. The native
XHR `upload.onprogress` reports **bytes sent**, not server decoding or OCR.
100% transmitted is not proof of successful storage. Cancellation aborts the
client request; if decoding/commit already started, refresh the document queue
to see whether a document was stored. Retry is explicit and may create another
document if the previous response was lost; upload is not idempotent.

## Image coordinates and template editing

The viewer uses the entire canonical original image in an SVG viewBox sized to
`width_px × height_px`. Rectangles are integer half-open `[x0,y0,x1,y1)` in the
**upright source raster**, never CSS pixels. CSS zoom/rotation/pan affect only the
view. Pointer positions are mapped through the SVG inverse screen matrix; boxes
remain correct after responsive resizing and 90° view rotations. Rotation does
not rotate OCR input. Re-upload an externally corrected scan for real geometry
changes. The viewer is not a deskew, perspective, detector or tiling pipeline.

- Select a field in the inspector or focus/click its rectangle.
- Drag its rectangle to move. Drag the bottom-right blue handle to resize.
- Field moves carry its ordered subcrops with it. Field shrinking cannot exclude
  existing subcrops. Subcrop moves/resizes are clamped inside the parent.
- **Lines / cells** switches pointer editing to ordered subcrops, avoiding an
  overlapping full-width line stealing a field drag. Their displayed numbers are
  inference order. The inspector JSON is the authoritative ordered array.
- The text representation is editable for exact coordinates/reordering. Apply
  lines or comb cells; they are mutually exclusive. Server strictly checks bounds,
  positive area, integer coordinates, type/ID rules and total inference crops.
- The inspector edits label, type, required/numeric flags and enum choices.
  Only explicit `numeric:true` enables O/I/l/S/B digit normalization. Names must
  not be indiscriminately normalized. Checkboxes always require human booleans.
- Create/delete ROI edits only the template draft, not model results.
- Focus a parent rectangle and use arrows for one original-pixel moves; Shift
  moves ten pixels. Numeric inputs provide keyboard-accessible geometry editing.
- Ctrl/⌘ Z undoes; Ctrl/⌘ Shift Z redoes; Ctrl/⌘ S saves using the entered new
  version. Hotkeys do not intercept native text-input undo. History is bounded
  to 50 editor snapshots. Reset reloads the document snapshot when one is open.

Import/export template JSON contains layout, not credentials. The browser rejects
malformed/duplicate/out-of-bounds rectangles and fills safe optional crop-array
defaults. The server remains authoritative for the complete strict schema.
Import loads an editor draft, not a trusted server version. Enter a new version
and Save. Duplicate ID/version is rejected rather than overwritten.

**Apply means re-upload.** Select the newly saved version in the upload selector
and upload the original/corrected scan again. Existing documents/jobs preserve
their immutable snapshots. The Studio explicitly labels editor previews that
differ from the open document. If dimensions differ it does not draw a misleading
overlay over the old image. Crops/evidence always remain from the old snapshot
until the new document is selected.

## Jobs and evidence

Explicitly confirm alignment against the **document's immutable** template.
Queue local OCR creates a genuine durable job with a new idempotency key.
Progress is authenticated `fetch` streaming SSE, not EventSource with query-token
authentication. Sequence IDs resume the durable event history after the
server's stream timeout. Field counts come from the actual job.
Stream failures surface visibly and can be refreshed; no fabricated interval
progress or synthetic OCR values exist.

Queued/running jobs can be cancelled; terminal failed/partial/needs_review jobs
can be explicitly requeued. Persistent failures quarantine matching fingerprints
until an audited admin reset or artifact/config/runtime change. No automatic
multi-model/resource downgrade retry exists. A field timeout can leave partial
raw lines, never a false complete transcription.

Green means explicitly human-reviewed, amber means needs review/unavailable,
red means validation issue. Colors have text equivalents and never represent
invented confidence. Confidence is **unavailable**, not a percentage.
Open the comparison drawer for actual raw, normalized candidate, reviewed value,
model identity, attempted crops, elapsed seconds and runtime provenance.
Unavailable alternate models are clearly identified, not populated with dummy
comparison candidates.

## Human review and exports

Raw model text is read-only. Save human correction makes an encrypted,
revision-checked append-only correction and does not overwrite original OCR.
Unsaved text is not erased by concurrent event/evidence refreshes; older
response revisions cannot replace a newer correction. Switching fields can
discard an unsaved input buffer: **save before selecting another field**.
Concurrent operator edits can produce a revision conflict; refresh evidence
and reconcile deliberately rather than assuming last-write-wins.

All fields, including optional blanks/unknown checkbox decisions, require
explicit human review before Finalize. Required values, date/IFSC/PAN/enum rules
and amount reconciliation must pass. IFSC/PAN formatting is not identity/bank
verification. Ambiguous dates should be corrected to ISO. A rerun invalidates
final approval and marks prior corrections stale; reconfirm every field.

Draft export is explicitly unverified; reviewed export is blocked until final
approval. JSON includes raw/model/review/provenance; CSV values are escaped
server-side for formula prefixes and carry review state. Downloaded files leave
the encrypted application boundary: protect, retain and delete them separately.
Deleting a document removes application rows/jobs/reviews, not forensic remnants
in SQLite WAL, filesystem snapshots, backups or browser/download memory.

## Settings

Admin endpoints generate controls from the actual backend JSON schema.
Basic exposes bounded runtime settings; Advanced adds read-only restart settings;
Expert includes actual help and explicit audited retention/quarantine actions.
Search filters real schema keys. There are no inert fake model controls.

Runtime saves use CAS revisions and apply to **new jobs only**. Authoritative
startup/environment overrides cannot be weakened. Export is redacted, including
restart settings for diagnosis. Import filters to real runtime keys and submits
the current server revision. Full YAML/security config remains an offline
operator/restart operation. No browser provider-key store or remote endpoint
form exists because no secure provider adapter has been shipped.
