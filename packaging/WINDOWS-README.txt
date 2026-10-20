ICR Studio - unsigned Windows x64 manual-review preview

Extract the entire ZIP first. Keep ICR-Studio.exe, _internal, RUN-STUDIO.cmd
and bundle-manifest.json together. Do not run inside the ZIP or copy only
the executable. No administrator rights or OCR models are required.
Follow your organization's policy for unsigned software; do not bypass it.

Double-click RUN-STUDIO.cmd. It initializes keys only when no secret file
exists, then starts the local server. Keep its console open.
Open http://127.0.0.1:8000 when the server is ready.

To unlock Studio, privately open:
  %LOCALAPPDATA%\ICR-Studio\var\windows-secrets.json
The ICR_TOKENS value is a JSON mapping stored as a string. Use only its token
key in Studio, not the whole file, mapping or ICR_KEYS encryption material.
Never share this file. Back up encryption keys separately from private data.
Use Synthetic gallery to try manual review. OCR weights are not bundled.
Stop the server with Ctrl-C.

For command-line use, open PowerShell in the extracted application folder:
  .\ICR-Studio.exe init-secrets
  .\ICR-Studio.exe serve
Initialization intentionally refuses to overwrite existing keys. After the
first initialization, use only serve. For a busy port, use:
  .\ICR-Studio.exe --port 8012 serve
and open http://127.0.0.1:8012 instead.

The workspace must be on a fixed local NTFS volume, outside the extracted
application directory. Do not delete the private workspace to upgrade.
This is a portable review prototype, not an installer or production OCR.
Full instructions:
https://github.com/abhays02/ICR-Studios/blob/main/docs/CONFIGURATION.md#windows-release-build
