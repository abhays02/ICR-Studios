NAYEN v1 Studio (ICR Studios) - unsigned Windows x64 manual-review prototype

NAYEN v1 is the ICR Studios engine family: Indra (perception) and Laya
(decisions). This package contains the Studio operator workspace and its
local review backend. It is a prototype for human review workflows: no OCR
weights are bundled, nothing downloads them at startup, and the Rust engine
is not loaded in this build. OCR jobs report honest failure evidence
(model_unavailable) until you provision approved weights yourself - they
never invent text.

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
Use the Synthetic gallery to try manual review; every fixture is fictional.
Stop the server with Ctrl-C.

For command-line use, open PowerShell in the extracted application folder:
  .\ICR-Studio.exe init-secrets
  .\ICR-Studio.exe serve
Initialization intentionally refuses to overwrite existing keys. After the
first initialization, use only serve. For a busy port, use:
  .\ICR-Studio.exe --port 8012 serve
and open http://127.0.0.1:8012 instead.

The workspace must be on a fixed local NTFS volume, outside the extracted
application directory. Symlinks, reparse points and hard-linked workspace
files are rejected, and private Windows DACLs (your account + SYSTEM only)
are applied. Do not delete the private workspace to upgrade.
This is a portable review prototype, not an installer or production OCR.

Brand icon: packaging/nayen-mark.svg in the repository; conversion steps to
nayen.ico are documented in that file's header (the .ico is not committed).

Full instructions:
https://github.com/abhays02/ICR-Studios/blob/main/docs/CONFIGURATION.md#windows-release-build
Studio documentation:
https://abhays02.github.io/ICR-Studios/studio-guide.html
