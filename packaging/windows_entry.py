"""Console entry point for the deliberately CPU/manual-review Windows bundle."""
import argparse
import base64
import csv
import ctypes
import json
import multiprocessing
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
import sys


RESOURCE_ROOT = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parents[1]))
SECRET_NAMES = ("ICR_KEYS", "ICR_ACTIVE_KEY", "ICR_TOKENS")


def reject_reparse(path):
    for candidate in (path, *path.parents):
        if candidate.exists() or candidate.is_symlink():
            attributes = candidate.lstat()
            if candidate.is_symlink() or getattr(attributes, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                raise ValueError("Workspace cannot traverse reparse points or symlinks")


def protect_workspace(path):
    """Apply actual Windows DACLs; POSIX chmod is not an ACL on Windows."""
    if os.name != "nt":
        raise RuntimeError("This launcher requires Windows on a local NTFS volume")
    path = Path(os.path.abspath(path))
    reject_reparse(path)
    if str(path).startswith("\\\\"):
        raise ValueError("Network workspaces are not supported")
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetVolumePathNameW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32]
    kernel.GetVolumeInformationW.argtypes = [
        ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_void_p,
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_uint32]
    kernel.GetDriveTypeW.argtypes = [ctypes.c_wchar_p]
    volume = ctypes.create_unicode_buffer(261)
    if not kernel.GetVolumePathNameW(str(path.parent), volume, len(volume)):
        raise ctypes.WinError(ctypes.get_last_error())
    filesystem = ctypes.create_unicode_buffer(261)
    if not kernel.GetVolumeInformationW(volume.value, None, 0, None, None, None,
                                        filesystem, len(filesystem)):
        raise ctypes.WinError(ctypes.get_last_error())
    if filesystem.value != "NTFS" or kernel.GetDriveTypeW(volume.value) != 3:
        raise ValueError("A local NTFS workspace is required for private Windows ACLs")
    whoami = Path(os.environ["SystemRoot"]) / "System32" / "whoami.exe"
    result = subprocess.run([str(whoami), "/user", "/fo", "csv", "/nh"],
                            check=True, capture_output=True, text=True)
    sid = next(csv.reader(result.stdout.splitlines()))[1]
    if not re.fullmatch(r"S-\d+(?:-\d+)+", sid):
        raise ValueError("Cannot determine the current Windows account SID")
    advapi = ctypes.WinDLL("advapi32", use_last_error=True)
    descriptor = ctypes.c_void_p()
    advapi.ConvertStringSecurityDescriptorToSecurityDescriptorW.argtypes = [
        ctypes.c_wchar_p, ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p), ctypes.c_void_p]
    advapi.SetFileSecurityW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_void_p]
    kernel.LocalFree.argtypes = [ctypes.c_void_p]
    sddl = f"D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"
    if not advapi.ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, 1,
                                                                       ctypes.byref(descriptor), None):
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        path.mkdir(parents=True, exist_ok=True)
        entries = [path]
        for parent, directories, files in os.walk(path, followlinks=False):
            for name in directories + files:
                entry = Path(parent) / name
                reject_reparse(entry)
                if entry.is_file() and entry.stat().st_nlink != 1:
                    raise ValueError("Workspace files cannot be hard links")
                entries.append(entry)
        for entry in entries:
            if not advapi.SetFileSecurityW(str(entry), 0x80000004, descriptor):
                raise ctypes.WinError(ctypes.get_last_error())
    finally:
        kernel.LocalFree(descriptor)
    return path


def initialize_secrets(path):
    values = {
        "ICR_KEYS": json.dumps({"k1": base64.b64encode(os.urandom(32)).decode("ascii")}),
        "ICR_ACTIVE_KEY": "k1",
        "ICR_TOKENS": json.dumps({secrets.token_urlsafe(32): {"owner": "operator", "admin": True}}),
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as stream:
        json.dump(values, stream)


def load_secrets(path):
    supplied = [name in os.environ for name in SECRET_NAMES]
    if any(supplied) and not all(supplied):
        raise ValueError("Supply all three secret environment variables, or none")
    if not any(supplied):
        values = json.loads(path.read_text(encoding="utf-8"))
        if set(values) != set(SECRET_NAMES) or not all(isinstance(value, str) for value in values.values()):
            raise ValueError("Invalid Windows secret file")
        os.environ.update(values)


def main(argv=None):
    parser = argparse.ArgumentParser(description="ICR Studio local manual-review preview; no OCR weights")
    parser.add_argument("--workspace", type=Path,
                        default=Path(os.environ.get("LOCALAPPDATA", "")) / "ICR-Studio")
    parser.add_argument("--port", type=int, default=8000)
    parser.add_argument("command", choices=("init-secrets", "serve"))
    args = parser.parse_args(argv)
    arguments = argv if argv is not None else sys.argv[1:]
    if not os.environ.get("LOCALAPPDATA") and not any(
            value == "--workspace" or value.startswith("--workspace=") for value in arguments):
        parser.error("LOCALAPPDATA is unavailable; specify an existing private local workspace parent")
    requested = Path(os.path.abspath(args.workspace))
    application_root = Path(sys.executable).parent if getattr(sys, "frozen", False) else RESOURCE_ROOT
    if requested == application_root or requested.is_relative_to(application_root):
        raise ValueError("Workspace must be outside bundled application resources")
    workspace = protect_workspace(requested)
    os.chdir(workspace)
    secret_file = workspace / "var" / "windows-secrets.json"
    if args.command == "init-secrets":
        initialize_secrets(secret_file)
        print("Created var/windows-secrets.json in the protected workspace. Back up keys separately.")
        return
    load_secrets(secret_file)
    from backend.app.config import load_settings
    from backend.app.main import create_app
    import uvicorn

    settings, _ = load_settings(RESOURCE_ROOT / "config" / "windows.yaml", {
        "mode": "local", "host": "127.0.0.1", "port": args.port, "data_dir": "var",
        "model_dir": "var/models-not-bundled", "device": "cpu", "cors_origins": [],
    })
    print(f"Manual-review preview at http://127.0.0.1:{args.port}; OCR is not bundled.")
    print("Windows decoder: input/pixel caps and wall deadline; no POSIX hard memory/CPU limits.")
    uvicorn.run(create_app(settings=settings), host=settings.host, port=settings.port,
                access_log=False, log_level="warning", limit_concurrency=32,
                loop="asyncio", http="h11", ws="none", lifespan="on")


if __name__ == "__main__":
    multiprocessing.freeze_support()
    try:
        main()
    except (ValueError, OSError, RuntimeError):
        print("Startup failed safely. Check workspace ACLs, secret configuration and port. "
              "Existing keys are never overwritten.", file=sys.stderr)
        raise SystemExit(1) from None
