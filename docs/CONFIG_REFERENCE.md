# Generated configuration reference

Regenerate: `icr config-reference`. Schema version 1. Unknown keys fail closed.
Precedence: defaults < YAML < permitted persisted runtime < environment < explicit startup.
Only max_tokens, attempt_seconds and job_seconds may change for new jobs.
Secrets: ICR_KEYS (JSON base64 keyring), ICR_ACTIVE_KEY, ICR_TOKENS (JSON token/principal map)
are environment-only, excluded from config/schema/export. ICR_CONFIG selects YAML.

| Key / environment | Default | Constraints / description |
| --- | --- | --- |
| `schema_version` / `ICR_SCHEMA_VERSION` | `1` | {"const": 1, "type": "integer"} |
| `mode` / `ICR_MODE` | `"local"` | {"enum": ["local", "server", "demo"], "type": "string"} |
| `host` / `ICR_HOST` | `"127.0.0.1"` | {"description": "Restart-only listen address.", "type": "string"} |
| `port` / `ICR_PORT` | `8000` | {"maximum": 65535, "minimum": 1024, "type": "integer"} |
| `data_dir` / `ICR_DATA_DIR` | `"var"` | {"description": "Project-relative beneath private var/; never serve statically.", "type": "string"} |
| `model_dir` / `ICR_MODEL_DIR` | `"var/models/microsoft-trocr"` | {"description": "Approved snapshot, never fetched on startup.", "type": "string"} |
| `device` / `ICR_DEVICE` | `"cpu"` | {"enum": ["cpu", "cuda", "mps", "auto"], "type": "string"} |
| `dtype` / `ICR_DTYPE` | `"float32"` | {"const": "float32", "type": "string"} |
| `local_only` / `ICR_LOCAL_ONLY` | `true` | {"const": true, "type": "boolean"} |
| `cors_origins` / `ICR_CORS_ORIGINS` | `[]` | {"items": {"type": "string"}, "type": "array"} |
| `upload_bytes` / `ICR_UPLOAD_BYTES` | `10485760` | {"maximum": 10485760, "minimum": 1024, "type": "integer"} |
| `max_pixels` / `ICR_MAX_PIXELS` | `25000000` | {"maximum": 25000000, "minimum": 100, "type": "integer"} |
| `pdf_pages` / `ICR_PDF_PAGES` | `1` | {"const": 1, "type": "integer"} |
| `pdf_dpi` / `ICR_PDF_DPI` | `150` | {"maximum": 200, "minimum": 72, "type": "integer"} |
| `max_fields` / `ICR_MAX_FIELDS` | `100` | {"maximum": 100, "minimum": 1, "type": "integer"} |
| `max_tokens` / `ICR_MAX_TOKENS` | `128` | {"maximum": 512, "minimum": 1, "type": "integer"} |
| `attempt_seconds` / `ICR_ATTEMPT_SECONDS` | `60` | {"maximum": 300, "minimum": 1, "type": "integer"} |
| `job_seconds` / `ICR_JOB_SECONDS` | `300` | {"maximum": 1800, "minimum": 1, "type": "integer"} |
| `retention_days` / `ICR_RETENTION_DAYS` | `7` | {"maximum": 365, "minimum": 1, "type": "integer"} |
| `worker_enabled` / `ICR_WORKER_ENABLED` | `true` | {"type": "boolean"} |
