"""Versioned strict settings; no secrets in settings files or exports."""
import json
import os
from pathlib import Path
from typing import Literal
from urllib.parse import urlsplit

import yaml
from pydantic import BaseModel, ConfigDict, Field, model_validator


class Settings(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True, frozen=True)
    schema_version: Literal[1] = 1
    mode: Literal["local", "server", "demo"] = "local"
    host: str = Field(default="127.0.0.1", description="Restart-only listen address.")
    port: int = Field(default=8000, ge=1024, le=65535)
    data_dir: str = Field(default="var", description="Project-relative beneath private var/; never serve statically.")
    model_dir: str = Field(default="var/models/microsoft-trocr", description="Approved snapshot, never fetched on startup.")
    device: Literal["cpu", "cuda", "mps", "auto"] = "cpu"
    dtype: Literal["float32"] = "float32"
    local_only: Literal[True] = True
    cors_origins: list[str] = Field(default_factory=list)
    upload_bytes: int = Field(default=10485760, ge=1024, le=10485760)
    max_pixels: int = Field(default=25000000, ge=100, le=25000000)
    pdf_pages: Literal[1] = 1
    pdf_dpi: int = Field(default=150, ge=72, le=200)
    max_fields: int = Field(default=100, ge=1, le=100)
    max_tokens: int = Field(default=128, ge=1, le=512)
    attempt_seconds: int = Field(default=60, ge=1, le=300)
    job_seconds: int = Field(default=300, ge=1, le=1800)
    retention_days: int = Field(default=7, ge=1, le=365)
    worker_enabled: bool = True

    @model_validator(mode="after")
    def security(self):
        if self.mode != "server" and self.host not in ("127.0.0.1", "::1", "localhost"):
            raise ValueError("Nonloopback binding requires server mode")
        for origin in self.cors_origins:
            parsed = urlsplit(origin)
            if (parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username
                    or parsed.password or parsed.path or parsed.query or parsed.fragment or "*" in origin
                    or (parsed.scheme == "http" and parsed.hostname not in ("localhost", "127.0.0.1", "::1"))
                    or (parsed.port is not None and not 1 <= parsed.port <= 65535)):
                raise ValueError("Explicit HTTPS or loopback development CORS origins required")
        path = Path(self.data_dir)
        if path.is_absolute() or ".." in path.parts or not path.parts or path.parts[0] != "var":
            raise ValueError("data_dir must be project-relative beneath private var/")
        return self


HOT_KEYS = frozenset({"max_tokens", "attempt_seconds", "job_seconds"})


def load_settings(path=None, overrides=None):
    values = {}
    sources = {k: "default" for k in Settings.model_fields}
    if path:
        values = yaml.safe_load(Path(path).read_text()) or {}
        if not isinstance(values, dict):
            raise ValueError("Config must be a mapping")
        sources.update({k: "yaml" for k in values})
    for key in Settings.model_fields:
        env = os.environ.get("ICR_" + key.upper())
        if env is not None:
            try:
                values[key] = json.loads(env)
            except json.JSONDecodeError:
                values[key] = env
            sources[key] = "environment"
    for key, value in (overrides or {}).items():
        values[key] = value
        sources[key] = "explicit"
    return Settings.model_validate(values), sources


def private_dir(path):
    path = Path(path)
    if any(parent.is_symlink() for parent in (path, *path.parents)):
        raise ValueError("Private directory cannot traverse symlinks")
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    path.chmod(0o700)
    return path
