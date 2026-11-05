# models/

ONNX weights for the browser engine go here, named exactly after the engine's
`ModelKind` (see `web/engine/src/types.ts`):

```
orient.onnx  det.onnx  script.onnx  rec.onnx
rec_world.onnx  rec_cjk.onnx  rec_rtl.onnx
meta.json
```

Fetch them from the upstream release rather than copying files by hand:

```
./scripts/fetch_indra_models --download --approve-license openrail --web
```

The page feature-detects what is present at load time and shows the count.
Minimum for real inference: `det.onnx` + `rec.onnx` (plus the wasm glue in
`../wasm/`). Missing optional models skip their stage and add a note to the
result; they are never faked.

Provenance: weights come from `datalab-to/indra-ocr` at a pinned revision,
under the OpenRAIL licence, and are **not** redistributed in this repository.
`order.onnx` is never fetched — reading order is computed deterministically.
This directory stays empty in git on purpose.
