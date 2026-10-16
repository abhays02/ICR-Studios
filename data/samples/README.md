# Fictional bank-form fixtures — DATA incomplete

Six actual **1800 × 2400 RGB PNGs** (200 DPI metadata) are provided, covering
CAR, LAR, KYC, RTGS and cheque, plus a noisy/sheared CAR with an invalid date.
The generated PNGs total approximately 1.2 MB; all data/fonts total under 3 MB.

**Every person, account, institution and address is fictional. These are
script-font simulations, NOT real handwriting, signed instruments, government
forms, OCR predictions or accuracy evidence.** KYC/RTGS/CHEQUE are fixture-only
extensions; initial backend support may remain CAR/LAR. CAR and LAR mean Customer
Account Registration and Loan Application Request, not regulatory standards.
No real customer data, financial credentials or model weights were used.

## Reproduce

From the repository root (Python 3.11+):

```sh
python -m pip install Pillow==12.3.0
./scripts/fetch_samples
./scripts/generate_samples --seed 20261015
python -m unittest discover -s tests -p 'test_samples.py' -v
```

Pillow 12.3.0 was checked against the GitHub advisory database before installation.
Generation is offline once the committed fonts are available; no need to fetch
again. `fetch_samples --fonts-only` provisions just fonts. Font provisioning
requires the local DejaVu Sans package and accompanying Debian copyright notice;
it does not silently substitute fonts. Font downloads are pinned to the full
Google Fonts revision recorded in `../fonts/manifest.json`. Same seed, script,
font bytes, Pillow version and rasterizer environment produce identical outputs.
Cross-platform FreeType/zlib changes may affect raster bytes.

## Studio/backend handoff

- `manifest.json`: image paths, hashes, sizes, dimensions, form kind, template path,
  per-image seed, origin/license, generator hash and font provenance.
- `../templates/{car,lar,kyc,rtgs,cheque}-v1.0.0.json`: `template_id`,
  `template_version`, `schema_version`, `form_kind`, `width_px`, `height_px`,
  and `fields[]` containing stable `field_id`, `label`, `type`, `required`,
  `page`, `roi`, `line_rois`. Treat versions as immutable once imported.
- `../schemas/*-v1.0.0.schema.json`: draft business-field JSON Schemas,
  `additionalProperties:false`; null is allowed for unrecognized/incomplete values.
  Final review must enforce nonnull required values and calendar/cross-field rules.
- `*.ground_truth.json`: separate **test-only** annotations, never an OCR provider.
  `fields` is keyed by `field_id`; `value` is typed, `raw_text` is the rendered
  text (null for checkbox), and blank optional tax identifier is `""`.
  `tenure_months` renders `"024"` but its typed business value is integer `24`.
  Accounts/identity/cheque references remain strings with leading zeros.
- All ROIs/bboxes use half-open `[x0,y0,x1,y1)` **original integer pixel**
  bounds, never CSS coordinates. Templates use the same bounds in template space.
  Multi-line address has two ordered line ROIs. Checkbox fields are manual-review
  annotations, not OCR-supported signature verification.
- Ground truth records `template_roi`, `template_line_rois`, template ink bounds,
  `original_polygon`, `original_bbox`, `original_line_polygons` and
  `original_line_bboxes`. `template_to_image` is an explicit 3×3 transform.
  First five samples have identity alignment; sample six has affine horizontal
  shear `x_image = x_template + 0.012*(y_template - 1200)`, and sparse scan noise.
  **Do not identity-crop sample six or use its enclosing bboxes as a perspective
  warp.** Initial backends should require explicit reviewed alignment or use the
  five identity samples. This is shear/skew augmentation, not camera homography.

The invalid sample intentionally contains `1991-02-30`. Optional blanks,
multiline text, checked/unchecked boxes and numeric comb boxes are included.
The declared test routing code is deliberately non-routable/format-invalid;
identifiers are not intended to pass any bank's real validation profile.

## Font rights and provenance

Caveat by Impallari Type / The Caveat Project Authors is a genuine
handwriting-*style font*, downloaded from the official Google Fonts repository
at `1d0e46c7933245c8180fe83e47917961c913bbdb`; its metadata identifies the
upstream Caveat revision `59745e818ef7973e11e70cb1358d0e902b56c5fc`.
The font is redistributed unmodified under **SIL OFL 1.1** with `Caveat-OFL.txt`
and original metadata. DejaVu Sans is an unmodified locally installed label font,
redistributed with `DejaVu-NOTICES.txt` under Bitstream Vera terms (DejaVu changes
public domain). Its source is local, not falsely claimed as a pinned download.
Every font/license file has its original source and SHA-256 in the font manifest.
Original generated forms/layouts/annotations have the CC0 dedication in
`LICENSE-CC0.txt`; that does not change third-party font/dataset rights.

## Real handwriting acquisition: unresolved, not fabricated

`../real_handwriting/fetch_report.json` is the actual attempt ledger.
On 2026-10-15 the real handwritten-digit dataset metadata endpoint
`https://huggingface.co/api/datasets/ylecun/mnist` failed DNS resolution
(`No address associated with hostname`). **No HF revision or dataset license
was verified, and no real handwritten image was downloaded. DATA is incomplete
for the real-handwriting requirement.** Digits alone would not establish cursive
or bank-domain accuracy even if acquired.

The official CVDF mirror README was successfully retrieved from GitHub at
`7477bafafc2029b41adb6b2ebf5033aeaacee71e`. It explicitly attributes the same
MNIST dataset to LeCun and contains only external Google Storage download links;
the GitHub repository contains no images or dataset-license grant. Thus no
unverified GitHub mirror or unrelated sample was copied, and external storage
was not used to bypass the requested GitHub-only fallback.

When connectivity returns, run **`./scripts/fetch_samples`** again. It resolves
and validates a full 40-hex HF dataset revision, retrieves the card at that
revision, requires its explicit `cc-by-sa-3.0` declaration, then downloads one
test Parquet shard capped at 5 MiB and records its SHA-256, source and attribution.
Changed/missing licensing fails closed. This is a raw digit test artifact, not
extracted line handwriting; no Parquet parser dependency is installed and it is
not mixed into synthetic fixtures. Inspect its license and contents before use.
Exit code 2 means the real-data acquisition remains incomplete; font provisioning
can still have succeeded. No dataset code is executed or credentials required.
