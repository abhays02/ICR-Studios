"""Operator-only lifecycle commands. Credentials never enter config exports."""
import argparse
import base64
import importlib.metadata
import json
import os
import secrets
from pathlib import Path

from .config import Settings, load_settings, private_dir


def main():
    parser = argparse.ArgumentParser(prog="icr")
    parser.add_argument("--config")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("init-secrets", help="Create private var/secrets.env once; never print secret values")
    sub.add_parser("serve")
    sub.add_parser("doctor")
    sub.add_parser("migrate", help="Create/validate schema v1; refuse unknown versions")
    sub.add_parser("rotate", help="Reencrypt all rows in restartable 50-row transactions")
    reference = sub.add_parser("config-reference")
    reference.add_argument("--output", default="docs/CONFIG_REFERENCE.md")
    args = parser.parse_args()
    settings, _ = load_settings(args.config or os.environ.get("ICR_CONFIG"))
    if args.command == "init-secrets":
        directory = private_dir(settings.data_dir)
        path = directory / "secrets.env"
        token = secrets.token_urlsafe(32)
        key = base64.b64encode(os.urandom(32)).decode()
        with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as stream:
            stream.write("export ICR_KEYS='" + json.dumps({"k1": key}) + "'\n")
            stream.write("export ICR_ACTIVE_KEY=k1\n")
            stream.write("export ICR_TOKENS='" + json.dumps({token: {"owner": "operator", "admin": True}}) + "'\n")
        print("Created private environment file; source it locally. Never commit or share it.")
    elif args.command == "serve":
        import uvicorn
        from .main import create_app
        # Disable access logs: query strings and uploaded identifiers must not enter logs.
        uvicorn.run(create_app(config_path=args.config), host=settings.host, port=settings.port,
                    access_log=False, log_level="warning", limit_concurrency=32)
    elif args.command == "doctor":
        report = {"python": os.sys.version.split()[0], "requested_device": settings.device,
                  "dtype": "float32", "cuda": False, "mps": False}
        for name in ("torch", "transformers"):
            try:
                report[name] = importlib.metadata.version(name)
            except importlib.metadata.PackageNotFoundError:
                report[name] = "not_installed"
        if report["torch"] != "not_installed":
            import torch
            report["cuda"] = torch.cuda.is_available()
            report["mps"] = hasattr(torch.backends, "mps") and torch.backends.mps.is_available()
        from .ocr import model_status
        report["model"] = model_status(settings.model_dir)
        print(json.dumps(report, indent=2))
    elif args.command == "config-reference":
        lines = ["# Generated configuration reference", "",
                 "Regenerate: `icr config-reference`. Schema version 1. Unknown keys fail closed.",
                 "Precedence: defaults < YAML < permitted persisted runtime < environment < explicit startup.",
                 "Only max_tokens, attempt_seconds and job_seconds may change for new jobs.",
                 "Secrets: ICR_KEYS (JSON base64 keyring), ICR_ACTIVE_KEY, ICR_TOKENS (JSON token/principal map)",
                 "are environment-only, excluded from config/schema/export. ICR_CONFIG selects YAML.", "",
                 "| Key / environment | Default | Constraints / description |", "| --- | --- | --- |"]
        for name, value in Settings.model_json_schema()["properties"].items():
            lines.append(f"| `{name}` / `ICR_{name.upper()}` | `{json.dumps(value.get('default', []))}` | "
                         + json.dumps({k: v for k, v in value.items() if k not in ("default", "title")}) + " |")
        Path(args.output).write_text("\n".join(lines) + "\n")
    else:
        from .crypto import Keyring
        from .db import Store
        store = Store(settings, Keyring.from_env())
        if args.command == "rotate":
            count = 0
            while rotated := store.rotate(50):
                count += rotated
            print(json.dumps({"rotated": count, "active_key": store.keys.active,
                              "warning": "Retain old keys for backups until separately audited."}))
        else:
            print("Schema v1 ready")


if __name__ == "__main__":
    main()
