# wasm/

The built engine glue lives here and is **not** committed — it is generated
from `crates/nayen-wasm`:

```
wasm/nayen_wasm.js          # wasm-bindgen ES module (--target web)
wasm/nayen_wasm_bg.wasm     # the engine: Indra + Laya, ~472 KB
```

Build it from the repository root:

```
./scripts/build_wasm
```

The script refuses to emit glue when the installed `wasm-bindgen` CLI does not
match the crate's exact pin, because a mismatched pair mis-handles strings.

If these files are absent the page says so and falls back to the clearly
labelled `SimulatedDriver`, which prints a visible warning rather than passing
itself off as a model run — see `web/src/engine/driver.ts`.
