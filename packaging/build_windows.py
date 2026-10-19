"""Build an allowlisted one-folder Windows bundle, never a workspace snapshot."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]


def resource_files():
    files = [(ROOT / "packaging/windows.yaml", "config")]
    for directory, patterns in (
        ("data/templates", ("*.json",)),
        ("data/schemas", ("*.json",)),
        ("data/samples", ("manifest.json", "*.png", "*.ground_truth.json", "LICENSE-CC0.txt")),
        ("studio/dist", ("**/*",)),
    ):
        for pattern in patterns:
            files.extend((path, str(path.parent.relative_to(ROOT)))
                         for path in sorted((ROOT / directory).glob(pattern)) if path.is_file())
    return files


def main():
    if sys.platform != "win32":
        raise SystemExit("Build on Windows; cross-compilation is not supported")
    if not (ROOT / "studio/dist/index.html").is_file():
        raise SystemExit("Build the offline Studio first")
    command = [
        sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", "--onedir",
        "--name", "ICR-Studio", "--noupx", "--console", "--paths", str(ROOT),
        "--distpath", str(ROOT / "dist"), "--workpath", str(ROOT / "var/windows-build"),
        "--specpath", str(ROOT / "var/windows-spec"),
        "--hidden-import", "uvicorn.logging", "--hidden-import", "uvicorn.loops.asyncio",
        "--hidden-import", "uvicorn.protocols.http.h11_impl",
        "--hidden-import", "uvicorn.lifespan.on",
        "--hidden-import", "sqlalchemy.dialects.sqlite",
        "--collect-all", "pypdfium2", "--collect-all", "pypdfium2_raw",
        "--copy-metadata", "Pillow", "--copy-metadata", "pypdfium2",
        "--exclude-module", "torch", "--exclude-module", "transformers",
        "--exclude-module", "pytest", "--exclude-module", "tkinter",
    ]
    for source, destination in resource_files():
        command.extend(("--add-data", f"{source}{os.pathsep}{destination}"))
    command.append(str(ROOT / "packaging/windows_entry.py"))
    subprocess.run(command, cwd=ROOT, check=True)
    bundle = ROOT / "dist/ICR-Studio"
    files = sorted(path for path in bundle.rglob("*") if path.is_file())
    forbidden = {"web_demo", ".git", "var", ".env", "windows-secrets.json", "secrets.env",
                 "icr.sqlite3", "torch", "transformers"}
    for path in files:
        if forbidden.intersection(path.relative_to(bundle).parts) or path.suffix in {".safetensors", ".pt", ".pth"}:
            raise SystemExit("Forbidden workspace, model or secret artifact in bundle")
    manifest = {"format": 1, "purpose": "Windows local CPU/manual-review preview; no OCR weights",
                "files": {path.relative_to(bundle).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                          for path in files}}
    (bundle / "bundle-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
