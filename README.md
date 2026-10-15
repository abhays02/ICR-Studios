# ICR-bank

Offline-first banking-form transcription **and human review**. The executable
backend supports encrypted uploads, immutable templates, durable jobs/events,
manual corrections and exports. The real local Microsoft TrOCR adapter requires
operator-approved offline weights; **no weights or measured recognition results
are included**. Other model candidates and remote providers remain unsupported.
The studio/deployment are subsequent implementation work.

```sh
uv sync --extra test
uv run icr init-secrets
. var/secrets.env
uv run icr serve
```

Default: authenticated loopback API at port8000. Never commit `var/secrets.env`
or browser-persist its token. See [API contract](docs/API.md),
[verified execution status/gaps](docs/STATUS.md),
[configuration reference](docs/CONFIG_REFERENCE.md) and
[model evidence](docs/MODELS.md). Fixtures are explicitly fictional
script-font simulations, not real handwriting or OCR output.
