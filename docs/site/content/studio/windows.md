# Windows build and run

The Windows artifact packages the Studio operator workspace plus the local
review backend into one unsigned, portable folder. It is a **manual-review
prototype**: no OCR weights are bundled, and nothing downloads them at
startup. Missing weights produce real failure evidence in the UI, never
sample-derived predictions.

## What the bundle is

- `ICR-Studio.exe` — PyInstaller one-folder build of the local backend and
  launcher (`init-secrets` / `serve`).
- `RUN-STUDIO.cmd` — initializes keys only when no secret file exists, then
  starts the loopback server.
- `_internal/` — runtime resources: `config/windows.yaml`, synthetic sample
  fixtures, template/schema JSON and the built Studio web assets
  (`studio/dist`), including the NAYEN v1 theme and self-hosted fonts, so the
  app renders identically offline.
- `bundle-manifest.json` — SHA-256 of every bundled file.
- `README.txt` — the operator instructions.

The build is allowlisted: only the files above are bundled, and the build
**fails** if any forbidden artifact (`.git`, `var`, `.env`, secret files,
SQLite databases, `.safetensors`/`.pt`/`.pth` weights, `torch`,
`transformers`) appears in the output. The NAYEN v1 Rust engine is not part
of this bundle yet; Studio shows an explicit **ENGINE NOT LOADED** state and
the local handwriting adapter fails honestly without provisioned weights.

## Build (on Windows)

Cross-compilation is not supported; run on Windows x64:

```sh
cd studio && npm ci --ignore-scripts && npm run build   # produces studio/dist
python -m pip install pyinstaller
python packaging\build_windows.py                        # produces dist/ICR-Studio
```

Icon: the bundle ships without a compiled `.ico` by default. The brand mark
is `packaging/nayen-mark.svg`; convert it once (documented in
`packaging/README` notes and the SVG header) to `packaging/nayen.ico`, and
`build_windows.py` picks it up automatically when present. No unverified
binary blobs are committed.

After the build, run the packaging smoke test on Windows CI:

```sh
python packaging\smoke_windows.py dist\ICR-Studio\ICR-Studio.exe
```

## Run

1. Extract the entire ZIP. Keep `ICR-Studio.exe`, `_internal`,
   `RUN-STUDIO.cmd` and `bundle-manifest.json` together. Do not run inside
   the ZIP.
2. Double-click `RUN-STUDIO.cmd`. It creates
   `%LOCALAPPDATA%\ICR-Studio\var\windows-secrets.json` on first run
   (initialization refuses to overwrite existing keys), then serves on
   `http://127.0.0.1:8000`. Keep the console open; Ctrl-C stops the server.
3. Unlock Studio with **only the operator token** from the private
   `windows-secrets.json` (`ICR_TOKENS` mapping key) — never the whole file
   and never the `ICR_KEYS` encryption material.
4. Try the manual-review workflow with the Synthetic gallery. OCR jobs will
   report `model_unavailable` failure evidence until you provision approved
   weights yourself.

Command-line use (PowerShell in the extracted folder):

```sh
.\ICR-Studio.exe init-secrets
.\ICR-Studio.exe serve
.\ICR-Studio.exe --port 8012 serve   # busy port
```

## Security boundaries (kept intact by design)

- The private workspace must live on a **local NTFS volume**, outside the
  extracted application directory; network paths and symlinks/reparse points
  are rejected, and real Windows DACLs (current SID + SYSTEM, full control)
  are applied to every workspace entry. Hard-linked workspace files are
  refused.
- Server binds `127.0.0.1` only; no CORS origins; concurrency-limited;
  no access log.
- Windows keeps bounded input/pixel limits and the decoder subprocess wall
  deadline, but does **not** provide POSIX hard memory/CPU resource limits —
  this is stated in `packaging/windows.yaml` rather than papered over.
- The bundle is unsigned. Follow your organization's policy for unsigned
  software; do not bypass it.

For the full release-build and provisioning context see
[CONFIGURATION.md](https://github.com/abhays02/ICR-Studios/blob/main/docs/CONFIGURATION.md#windows-release-build)
in the repository.
