# Profiles and parameters

A `Profile` is a named, reproducible configuration of detection and crop
geometry. Switching profiles **never touches recognizer weights** — it changes
how lines are found and cut, so the same models can be compared honestly on
the same pages.

> The shipped Indra defaults reproduce the reference pipeline exactly: a
> NAYEN run with `Profile::Indra` is bit-for-bit the reference. The
> handwriting values are **candidates, not proven wins**; `Profile` exists so
> the difference is measurable — run both over the same ground-truth pages
> with `nayen-cli eval` instead of trusting a constant in a source file.

## Profiles

| Profile | Intent | Detection preset | Crop preset |
| --- | --- | --- | --- |
| `indra` | Reference pipeline, unmodified. Default everywhere. | thresh 0.5 · min_area 2 · no box growth · long_side 1024 · adaptive re-detect on | pad_y 0.2 |
| `handwriting` | Faint pencil/ballpoint strokes, short words, ascenders outside the print-fitted core band. | thresh 0.3 · min_area 1 · grow_y 0.12 · grow_x 0.08 · long_side 1024 · adaptive on | pad_y 0.35 + contrast stretch |
| `sweep` | Multi-scale detection sweep with merged boxes; one extra detector pass per scale, opt-in. | thresh 0.35 · min_area 1 · grow_y 0.1 · grow_x 0.06 · long_side 1024 · adaptive off | pad_y 0.2 |

`Profile::describes()` returns these summaries at runtime for telemetry and UI
display — the Studio's engine strip shows the same text, not a paraphrase.

## `DetectParams` fields

| Field | Meaning |
| --- | --- |
| `thresh` | Sigmoid threshold on the core channel. Lower keeps fainter strokes. |
| `min_area` | Connected components smaller than this are dropped. |
| `grow_y` / `grow_x` | Grow each box by this fraction of its own height, per side, absorbing the ±1px error of the half-resolution output map. |
| `long_side` | Long side the page is scaled to before detection. |
| `adaptive` | Allow one re-detection pass at a larger scale for small text. A re-detected pass only wins if it finds more lines. |

## `EngineConfig`

Native (`crates/nayen-core/src/params.rs`) — the full surface:

| Field | Indra default | Browser preset | Notes |
| --- | --- | --- | --- |
| `profile` | `Indra` | as requested | |
| `detect` / `crop` | profile presets | profile presets | |
| `min_conf` | 0.5 | 0.5 | Below this mean CTC confidence a line yields **no text**; its box is kept as layout evidence. |
| `orient` | true | true | Skipped when the host did not load `orient.onnx`. |
| `deskew` | true | true | ~90 rotations of a thumbnail; the one stage a constrained host may want off. |
| `skew_side` | 800 | 480 | Deskew search thumbnail. |
| `route_scripts` | true | true | Skipped when only the core pair is loaded. |
| `laya` | true | true | Field extraction and cross-checks after recognition. |
| `max_page_side` | 2600 | 1800 | Bounds memory; in the browser a large scan would otherwise exhaust the WASM heap. |

The TypeScript mirror in `engine/src/types.ts` exposes the host-relevant
subset (`profile`, `minConf`, `orient`, `routeScripts`, `laya`,
`maxPageSide`) with `DEFAULT_CONFIG = { profile: 'indra', minConf: 0.5,
orient: true, routeScripts: true, laya: true, maxPageSide: 1800 }`.

## Structural pipeline constants

Fixed by training geometry — **not** profile-tunable:

| Constant | Value | Role |
| --- | --- | --- |
| `CORE_FRAC` | 0.4 | Core band fraction of a line's height used for confidence. |
| `CORE_X_SHRINK` | 0.15 | Horizontal shrink of the core band. |
| `OUT_STRIDE` | 2 | Detector output stride (half resolution). |
| `INPUT_MULTIPLE` | 32 | Detector input size multiple. |
| `LINE_HEIGHT` | 32 | Crop height every recognizer was trained on. |
| `WIDTH_MULTIPLE` | 8 | Recognizer timestep stride for width padding. |
| `MAX_CROP_WIDTH` | 4096 | Beyond this a “line” is too wide to be one line. |

## Decision thresholds

| Constant | Value | Role |
| --- | --- | --- |
| `MIN_CONF` | 0.5 | CTC confidence floor for emitting text. |
| `ROUTE_SURE` | 0.95 | Script classifier confidence for single-group routing; below it, the base recognizer also reads the line and the more confident result wins. |
| `ORIENT_SIDE` | 768 | Orientation classifier input side. |
| `ORIENT_MIN` | 0.95 | Quarter turns are applied only above this — a wrong rotation loses the whole page. |
| `ADAPTIVE_MIN_H` / `ADAPTIVE_TARGET_H` | 9.0 / 12.0 | Median line height below `MIN_H` at base resolution triggers one larger-scale re-detect; the pass must find at least `ADAPTIVE_MIN_BOXES = 8` boxes to win. |
| `ADAPTIVE_MAX_SIDE` | 2048 | Ceiling for the adaptive pass. |
| `PREVIEW_SIDE` / `SKEW_SIDE` | 1600 / 800 | Thumbnails the skew search and orient model read; never the full scan. |
| `DESKEW_MAX` / `DESKEW_MIN` / `DESKEW_COARSE_STEP` | 45.0 / 0.3 / 2.0 | Tilt range searched; below the floor a measured tilt is left alone (fine step 0.1 makes smaller values indistinguishable from noise). |
| `SPLIT_GAP` / `CELL_GAP` / `GUTTER_GAP` | 3.0 / 1.8 / 1.6 | CTC character-centre gaps (in line heights) used to split one detected line into separate fields / comb cells. |
| `MAX_BATCH` / `MAX_WIDTH_RATIO` / `MIN_BATCH_WIDTH` / `MAX_BATCH_AREA` | 64 / 1.3 / 64 / 40960 | Recognition batching limits. |
