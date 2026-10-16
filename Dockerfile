FROM node:24.21.0-bookworm-slim AS studio
WORKDIR /build/studio
COPY studio/package.json studio/package-lock.json ./
RUN npm ci --ignore-scripts
COPY studio/ ./
RUN npm run build

FROM python:3.12.3-slim-bookworm AS base
COPY --from=ghcr.io/astral-sh/uv:0.13.1 /uv /usr/local/bin/uv
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_NO_CACHE=1 \
    PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 \
    HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 HF_HUB_DISABLE_TELEMETRY=1
WORKDIR /app
ENV TMPDIR=/app/var/build
COPY pyproject.toml uv.lock ./
COPY backend/ backend/
COPY data/templates/ data/templates/
COPY data/schemas/ data/schemas/
COPY data/samples/ data/samples/
COPY config/ config/
COPY scripts/download_models scripts/benchmark scripts/
RUN mkdir -p var/build && uv sync --frozen --no-dev
COPY --from=studio /build/studio/dist studio/dist/
RUN groupadd --gid 10001 icr && useradd --uid 10001 --gid 10001 --no-create-home icr \
    && mkdir -p var && chown 10001:10001 var
ENV PATH="/app/.venv/bin:$PATH"
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=3s --start-period=20s \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8000/healthz',timeout=2)" || exit 1

FROM base AS inference
USER root
RUN uv sync --frozen --no-dev --extra inference
USER 10001:10001
CMD ["icr", "serve"]

FROM base AS cpu
USER 10001:10001
CMD ["icr", "serve"]
