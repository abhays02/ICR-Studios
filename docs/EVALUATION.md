# Evaluation protocol and honest current results

## Current evidence

**No real handwriting recognition metrics are available.** No weights were
downloaded, no approved weight revision/license was established, and the attempted
real-handwriting fetch remains incomplete after DNS failures. See
`data/real_handwriting/fetch_report.json`, [MODELS.md](MODELS.md) and [STATUS.md](STATUS.md).
DNS failure is not evidence that a requested model/dataset is absent.

Six local fixtures are fictional script-font simulations for geometry, schema,
queue, security and review plumbing only. Their ground-truth JSON is evaluation
reference, never model output. Backend sample import/worker do not load it.
MNIST digits, if obtained later, are not cursive/bank-form validation.

| Measurement | Current result |
| --- | --- |
| Real handwritten CER / WER / exact match | Not measured |
| Bank-domain, Indian names/addresses/IFSC/PAN/amounts accuracy | Not measured |
| Model confidence calibration / thresholds | Unavailable; no invented score |
| Cold/warm recognition latency, RAM/VRAM, throughput | Not measured |
| Optional Torch/Transformers model compatibility | Not installed/inference-tested |
| CUDA/MPS execution / NVIDIA image | Not tested |
| Real encrypted API / manual review / missing-weight jobs | Exercised |
| Authenticated durable SSE / draft/reviewed exports | Exercised |
| Original-pixel ROI drag + zoom/90° resize in real Chromium | Exercised |
| CPU Docker build/run + same browser workflows | Exercised |

A frontend build, model metadata lookup, artifact-presence check, framework
import or test-double success must not be counted as real OCR performance.
Browser test timings are not recognition-latency benchmarks. Actual missing-model
attempt timings are failure evidence, not successful inference timing.

## Running the real benchmark later

`scripts/benchmark --help` describes the actual executable benchmark. Supply an
evaluation-only manifest of consented local **line crops** and references:

```sh
uv sync --frozen --extra inference
uv run --frozen --extra inference python scripts/download_models --help
# Explicit approved immutable snapshot provisioning + offline verification first.
uv run --frozen --extra inference python scripts/benchmark --manifest path/to/evaluation-only-line-labels.json
```

Run from repository root with the locked optional runtime installed (or use its
virtualenv interpreter per the script help). The benchmark performs real bounded
local inference and reports CER, WER, exact match and per-sample cold/warm timing
plus hardware/device evidence. Failures emit `measurements:null`; never replace
failures with synthetic scores. The exact manifest shape/arguments in the script
are authoritative. Do not substitute fabricated ground truth into production
OCR endpoints to “complete” a run.

## Dataset design and reporting

Obtain explicit legal access/consent and model/data licenses before acquisition.
Record immutable revisions, SHA256 inventory, license approvals and provenance.
Keep evaluation references separate from production inference/caches; never use
ground-truth labels as a fallback recognizer. No customer PII should enter
committed fixtures, ordinary logs, screenshots or public benchmark reports.

Split by writer/source/institution/template where possible; avoid near-duplicate
train/test scans. Include names, address lines, dates, comb digits, IFSC/PAN
formats, amount words/figures with Indian grouping, optional blank/ambiguous
fields, checkboxes and rejected geometric distortions. Report language/script,
legibility, scan quality, deskew/alignment policy and excluded cases. The current
adapter requires ordered manually reviewed crops; it is not a line detector or
automatic tiling/preprocessing system.

Report sample count and denominator for every metric; retain null/failed/
unsupported outcomes and truncation/partial-line evidence. Publish both raw and
explicitly normalized metrics, documenting exactly which numeric-only
transformations were allowed. Do not spell-correct names to inflate accuracy.
Dates/IFSC/PAN validation checks are format constraints, not identity correctness.
Amount agreement does not establish that either transcription is correct.

Report exact-match and character/word edit metrics by field group plus aggregate,
with uncertainty intervals when justified. Record hardware, OS/runtime lock hash,
model artifact hash/revision, device/dtype/attention/quantization, warm-up rules,
attempt/job limits, cold/warm latency distributions, failures and peak resource
measurements. Do not infer VRAM minima from another project's throughput benchmark.

## Review and confidence

This release always requires human review and exposes unavailable confidence.
Green/amber/red are review/validation states, not probabilities. No sequence
confidence, calibrated threshold or multi-model agreement is asserted.
Future confidence work needs a disjoint labeled calibration set, explicit
coverage/error/abstention metrics and bank-domain human-review evaluation; a
model's self-assigned percentage is not acceptable.

Manual review completion and immutable raw/correction provenance are verifiable
workflow outcomes, not OCR accuracy. Synthetic browser smoke explicitly enters
human test values and proves final-review gates/exports, not correct recognition.
