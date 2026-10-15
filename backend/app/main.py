"""Authenticated API. Run via `icr serve`; importing never reads keys or loads weights."""
import asyncio
import base64
import csv
import hashlib
import hmac
import io
import json
import os
import time
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import Depends, FastAPI, HTTPException, Request
from fastapi.exceptions import RequestValidationError
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse, Response, StreamingResponse
from sqlalchemy import delete, select
from starlette.exceptions import HTTPException as StarletteHTTPException

from .config import HOT_KEYS, Settings, load_settings
from .crypto import IntegrityError, Keyring
from .db import Event, Job, Record, Store, uid
from .images import ImageRejected, bounded_canonical, crop
from .ledger import Ledger, fingerprint
from .limits import JSONLimit
from .ocr import CANDIDATES, model_status
from .schemas import Correction, JobRequest, Revision, SettingsPatch, Template
from .validation import normalize, reconcile
from .worker import TERMINAL, Worker

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data"


def error(code, status=422):
    raise HTTPException(status, code)


def principals_from_env():
    principals = json.loads(os.environ.get("ICR_TOKENS", "{}"))
    if not principals:
        raise ValueError("ICR_TOKENS required in all modes")
    for token, principal in principals.items():
        if (len(token) < 32 or set(principal) != {"owner", "admin"}
                or type(principal["admin"]) is not bool
                or not isinstance(principal["owner"], str)
                or not principal["owner"].isalnum() or len(principal["owner"]) > 64):
            raise ValueError("Invalid ICR_TOKENS principal")
    return principals


def builtin_templates():
    output = []
    for path in sorted((DATA / "templates").glob("*.json")):
        template = Template.model_validate_json(path.read_text())
        # The original fixture comb labels explicitly denote numeric-only boxes.
        for field in template.fields:
            if field.type == "comb":
                field.numeric = True
        schema_path = DATA / "schemas" / f"{template.form_kind.lower()}-v1.0.0.schema.json"
        if schema_path.exists():
            properties = json.loads(schema_path.read_text()).get("properties", {})
            for field in template.fields:
                choices = properties.get(field.field_id, {}).get("enum", [])
                field.choices = [v for v in choices if isinstance(v, str)]
        output.append(template.model_dump())
    return output


def create_app(settings=None, keys=None, principals=None, worker_target=None, config_path=None):
    if settings is None:
        settings, sources = load_settings(config_path or os.environ.get("ICR_CONFIG"))
    else:
        sources = {k: "explicit" if k in settings.model_fields_set else "default" for k in Settings.model_fields}
    keys = keys or Keyring.from_env()
    principals = principals if principals is not None else principals_from_env()
    store = Store(settings, keys)
    templates = builtin_templates()
    worker = Worker(store, settings, **({"child_target": worker_target} if worker_target else {}))

    def prune():
        cutoff = time.time() - settings.retention_days * 86400
        with store.transaction() as session:
            docs = session.scalars(select(Record).where(Record.kind == "document", Record.created < cutoff)).all()
            for document in docs:
                remove_document(session, document)

    @asynccontextmanager
    async def lifespan(app):
        prune()
        stop = asyncio.Event()

        async def retention_loop():
            while not stop.is_set():
                try:
                    await asyncio.wait_for(stop.wait(), timeout=3600)
                except asyncio.TimeoutError:
                    await asyncio.to_thread(prune)

        task = asyncio.create_task(retention_loop())
        if settings.worker_enabled:
            worker.start()
        yield
        stop.set()
        await task
        await asyncio.to_thread(worker.close)
        store.engine.dispose()

    app = FastAPI(title="ICR-bank", version="0.1.0", lifespan=lifespan, docs_url=None, redoc_url=None,
                  openapi_url=None)
    app.state.store, app.state.worker = store, worker
    decode_slot = asyncio.Semaphore(1)
    app.add_middleware(JSONLimit)
    app.add_middleware(CORSMiddleware, allow_origins=settings.cors_origins,
                       allow_methods=["GET", "POST", "PATCH", "DELETE"],
                       allow_headers=["Authorization", "Content-Type", "Last-Event-ID"],
                       expose_headers=["X-Request-ID"])

    @app.middleware("http")
    async def security_headers(request, call_next):
        request.state.request_id = uid()
        response = await call_next(request)
        response.headers.update({"Cache-Control": "no-store", "X-Content-Type-Options": "nosniff",
                                 "X-Request-ID": request.state.request_id,
                                 "Referrer-Policy": "no-referrer"})
        return response

    @app.exception_handler(StarletteHTTPException)
    async def http_error(request, exc):
        # Never reflect URL, token, validation inputs or arbitrary exception bodies.
        code = exc.detail if isinstance(exc.detail, str) and len(exc.detail) < 100 else "request_error"
        return JSONResponse({"code": code, "message": code.replace("_", " "),
                             "request_id": request.state.request_id, "details": []}, status_code=exc.status_code)

    @app.exception_handler(RequestValidationError)
    async def validation_error(request, exc):
        return JSONResponse({"code": "invalid_request", "message": "Request validation failed",
                             "request_id": request.state.request_id,
                             "details": [{"path": list(e["loc"][:-1]) + ["<unknown>"] if e["type"] == "extra_forbidden"
                                          else list(e["loc"]), "type": e["type"]} for e in exc.errors()][:20]},
                            status_code=422)

    @app.exception_handler(IntegrityError)
    async def integrity_error(request, exc):
        return JSONResponse({"code": "encrypted_data_integrity", "message": "Encrypted data unavailable",
                             "request_id": request.state.request_id, "details": []}, status_code=503)

    @app.exception_handler(Exception)
    async def unexpected_error(request, exc):
        return JSONResponse({"code": "internal_error", "message": "Operation failed safely",
                             "request_id": request.state.request_id, "details": []}, status_code=500)

    def auth(request: Request):
        header = request.headers.get("authorization", "")
        token = header[7:] if header.startswith("Bearer ") else ""
        selected = None
        for secret, principal in principals.items():
            if hmac.compare_digest(token.encode(), secret.encode()):
                selected = principal
        if selected is None:
            error("authentication_required", 401)
        return selected

    def admin(principal=Depends(auth)):
        if not principal["admin"]:
            error("admin_required", 403)
        return principal

    def get_document(session, document_id, principal):
        document = store.owned(session, document_id, principal["owner"], "document")
        if not document:
            error("document_not_found", 404)
        return document

    def get_job(session, job_id, principal):
        job = session.scalar(select(Job).where(Job.id == job_id, Job.owner == principal["owner"]))
        if not job:
            error("job_not_found", 404)
        return job

    def all_templates(session, principal):
        custom = session.scalars(select(Record).where(Record.kind == "template", Record.owner == principal["owner"])).all()
        return templates + [store.payload(row) for row in custom]

    def find_template(session, principal, template_id, version):
        match = next((t for t in all_templates(session, principal)
                      if t["template_id"] == template_id and t["template_version"] == version), None)
        if not match:
            error("template_not_found", 404)
        return match

    def effective(session):
        row = session.get(Record, "__settings")
        values = store.payload(row) if row else {}
        result = settings.model_dump()
        for key, value in values.items():
            if sources[key] not in ("environment", "explicit"):
                result[key] = value
        return Settings.model_validate(result), row.version if row else 0, values

    def fields_result(session, document):
        data = store.payload(document)
        latest = session.scalar(select(Job).where(Job.document_id == document.id,
                                                  Job.owner == document.owner).order_by(Job.created.desc()).limit(1))
        candidate = store.payload(latest).get("fields", {}) if latest else {}
        fields = {}
        for spec in data["template"]["fields"]:
            fid = spec["field_id"]
            field = dict(candidate.get(fid, {"field_id": fid, "raw_text": None, "normalized_value": None,
                                            "confidence": None, "confidence_kind": "unavailable",
                                            "status": "needs_review", "issues": ["unknown"],
                                            "bbox": spec["roi"], "line_rois": spec["line_rois"],
                                            "model_used": None}))
            review = data.get("corrections", {}).get(fid)
            value = review["value"] if review else field["normalized_value"]
            normalized, transformations, issues = normalize(value, spec)
            field.update({"value": normalized, "review": review, "validation_issues": issues,
                          "human_review_required": True})
            if review:
                field["status"] = "needs_review" if review.get("stale") else "reviewed"
            fields[fid] = field
        reconciliation = reconcile(fields, data["template"]["fields"])
        return {"schema_version": "1.0.0", "document_id": document.id,
                "form_kind": data["template"]["form_kind"], "revision": document.version,
                "template_id": data["template"]["template_id"],
                "template_version": data["template"]["template_version"],
                "review_state": data.get("review_state", "draft"), "fields": fields,
                "car_lar_match": reconciliation["match"], "amount_reconciliation": reconciliation,
                "job_id": latest.id if latest else None, "job_state": latest.state if latest else None,
                "warnings": ["Human review is required. No calibration or bank-domain accuracy established."]}

    def remove_document(session, document):
        jobs = session.scalars(select(Job).where(Job.document_id == document.id, Job.owner == document.owner)).all()
        for job in jobs:
            session.execute(delete(Event).where(Event.job_id == job.id))
            session.delete(job)
        for review in session.scalars(select(Record).where(Record.kind == "review", Record.owner == document.owner)).all():
            if store.payload(review)["document_id"] == document.id:
                session.delete(review)
        session.delete(document)

    @app.get("/healthz")
    def health():
        return {"status": "alive"}

    @app.get("/readyz")
    def ready():
        with store.sessions() as session:
            session.execute(select(Record.id).limit(1))
        return {"api": "ready", "inference": model_status(settings.model_dir)["status"],
                "human_review_available": True, "worker": "enabled" if settings.worker_enabled else "disabled"}

    @app.get("/api/v1/openapi.json")
    def openapi(principal=Depends(auth)):
        return app.openapi()

    @app.get("/api/v1/templates")
    def list_templates(principal=Depends(auth)):
        with store.sessions() as session:
            return {"items": all_templates(session, principal)}

    @app.post("/api/v1/templates", status_code=201)
    def add_template(body: Template, principal=Depends(auth)):
        with store.transaction() as session:
            data = body.model_dump()
            if any(t["template_id"] == body.template_id and t["template_version"] == body.template_version
                   for t in all_templates(session, principal)):
                error("immutable_template_version_conflict", 409)
            store.record(session, principal["owner"], "template", data)
            return data

    @app.get("/api/v1/templates/{template_id}/versions/{version}")
    def get_template(template_id: str, version: str, principal=Depends(auth)):
        with store.sessions() as session:
            return find_template(session, principal, template_id, version)

    @app.get("/api/v1/samples")
    def samples(principal=Depends(auth)):
        manifest = json.loads((DATA / "samples/manifest.json").read_text())
        return {"synthetic": True, "is_real_handwriting": False, "items": [
            {k: s[k] for k in ("sample_id", "form_kind", "width_px", "height_px", "alignment_required")}
            for s in manifest["samples"]]}

    def sample_entry(sample_id):
        items = json.loads((DATA / "samples/manifest.json").read_text())["samples"]
        entry = next((s for s in items if s["sample_id"] == sample_id), None)
        if not entry:
            error("sample_not_found", 404)
        return entry

    @app.get("/api/v1/samples/{sample_id}/image")
    def sample_image(sample_id: str, principal=Depends(auth)):
        entry = sample_entry(sample_id)
        return Response((DATA / "samples" / entry["image"]).read_bytes(), media_type="image/png")

    def save_document(session, principal, image, width, height, template, synthetic=False):
        if (width, height) != (template["width_px"], template["height_px"]):
            error("template_dimensions_mismatch", 422)
        row = store.record(session, principal["owner"], "document",
                           {"image": base64.b64encode(image).decode(), "width_px": width, "height_px": height,
                            "template": template, "corrections": {}, "review_state": "draft", "synthetic": synthetic})
        session.flush()
        return {"id": row.id, "revision": row.version, "width_px": width, "height_px": height,
                "template_id": template["template_id"], "template_version": template["template_version"],
                "review_state": "draft", "synthetic": synthetic}

    @app.post("/api/v1/samples/{sample_id}/import", status_code=201)
    def import_sample(sample_id: str, principal=Depends(auth)):
        entry = sample_entry(sample_id)
        if entry["alignment_required"]:
            error("reviewed_alignment_not_supported", 422)
        template = Template.model_validate_json((DATA / "samples" / entry["template"]).read_text())
        template_data = next(t for t in templates if t["template_id"] == template.template_id)
        with store.transaction() as session:
            return save_document(session, principal, (DATA / "samples" / entry["image"]).read_bytes(),
                                 entry["width_px"], entry["height_px"], template_data, True)

    @app.post("/api/v1/documents", status_code=201)
    async def upload(request: Request, template_id: str, template_version: str, principal=Depends(auth)):
        media = request.headers.get("content-type", "").split(";")[0].strip().lower()
        if media not in ("image/png", "image/jpeg", "application/pdf"):
            error("unsupported_media_type", 415)
        with store.sessions() as session:
            template = find_template(session, principal, template_id, template_version)
        raw = bytearray()
        try:
            async with asyncio.timeout(30):
                async for chunk in request.stream():
                    if len(raw) + len(chunk) > settings.upload_bytes:
                        error("upload_byte_limit", 413)
                    raw.extend(chunk)
        except TimeoutError:
            error("request_deadline", 408)
        try:
            await asyncio.wait_for(decode_slot.acquire(), timeout=.1)
        except TimeoutError:
            error("image_decoder_busy", 429)
        try:
            image, width, height = await asyncio.to_thread(bounded_canonical, bytes(raw), media, settings.max_pixels, settings.pdf_dpi)
        except ImageRejected:
            error("invalid_or_oversized_image", 422)
        finally:
            decode_slot.release()
        with store.transaction() as session:
            return save_document(session, principal, image, width, height, template)

    @app.get("/api/v1/documents")
    def documents(principal=Depends(auth), offset: int = 0, limit: int = 50):
        if offset < 0 or not 1 <= limit <= 100:
            error("invalid_pagination")
        with store.sessions() as session:
            rows = session.scalars(select(Record).where(Record.kind == "document", Record.owner == principal["owner"])
                                   .order_by(Record.created.desc()).offset(offset).limit(limit)).all()
            items = []
            for row in rows:
                data = store.payload(row)
                items.append({"id": row.id, "revision": row.version, "created_at": row.created,
                              "width_px": data["width_px"], "height_px": data["height_px"],
                              "template_id": data["template"]["template_id"],
                              "template_version": data["template"]["template_version"],
                              "review_state": data["review_state"], "synthetic": data["synthetic"]})
            return {"items": items}

    @app.get("/api/v1/documents/{document_id}")
    def document_detail(document_id: str, principal=Depends(auth)):
        with store.sessions() as session:
            row = get_document(session, document_id, principal)
            data = store.payload(row)
            return {"id": row.id, "revision": row.version, "template": data["template"],
                    "width_px": data["width_px"], "height_px": data["height_px"],
                    "review_state": data["review_state"], "synthetic": data["synthetic"]}

    @app.get("/api/v1/documents/{document_id}/image")
    def image(document_id: str, principal=Depends(auth)):
        with store.sessions() as session:
            row = get_document(session, document_id, principal)
            return Response(base64.b64decode(store.payload(row)["image"]), media_type="image/png")

    @app.get("/api/v1/documents/{document_id}/crops/{field_id}")
    def field_crop(document_id: str, field_id: str, principal=Depends(auth)):
        with store.sessions() as session:
            data = store.payload(get_document(session, document_id, principal))
            spec = next((f for f in data["template"]["fields"] if f["field_id"] == field_id), None)
            if not spec:
                error("field_not_found", 404)
            return Response(crop(base64.b64decode(data["image"]), spec["roi"]), media_type="image/png")

    @app.delete("/api/v1/documents/{document_id}", status_code=204)
    def delete_document(document_id: str, principal=Depends(auth)):
        with store.transaction() as session:
            remove_document(session, get_document(session, document_id, principal))

    @app.post("/api/v1/documents/{document_id}/jobs", status_code=202)
    def start_job(document_id: str, body: JobRequest, principal=Depends(auth)):
        with store.transaction() as session:
            document = get_document(session, document_id, principal)
            digest = fingerprint({"document": document_id, "request": body.model_dump()})
            old = session.scalar(select(Job).where(Job.owner == principal["owner"],
                                                   Job.idempotency == body.idempotency_key))
            if old:
                if old.digest != digest:
                    error("idempotency_conflict", 409)
                return {"id": old.id, "state": old.state}
            if session.scalar(select(Job.id).where(Job.document_id == document_id,
                                                   Job.state.in_(("queued", "running")))):
                error("document_job_active", 409)
            if session.scalar(select(Job.id).where(Job.state == "queued").offset(99).limit(1)):
                error("queue_full", 429)
            effective_settings, generation, _ = effective(session)
            data = store.payload(document)
            if len(data["template"]["fields"]) > effective_settings.max_fields:
                error("field_limit")
            job = Job(id=uid(), owner=principal["owner"], document_id=document_id,
                      idempotency=body.idempotency_key, digest=digest, state="queued", fence=0, lease=0)
            store.save(job, {"template": data["template"], "settings": effective_settings.model_dump(),
                             "config_generation": generation, "fields": {}, "request": body.model_dump()})
            session.add(job)
            store.event(session, job, "queued")
            data["review_state"] = "draft"
            for correction in data["corrections"].values():
                correction["stale"] = True
            document.version += 1
            store.save(document, data)
            return {"id": job.id, "state": "queued"}

    @app.get("/api/v1/jobs/{job_id}")
    def job_detail(job_id: str, principal=Depends(auth)):
        with store.sessions() as session:
            job = get_job(session, job_id, principal)
            data = store.payload(job)
            return {"id": job.id, "document_id": job.document_id, "state": job.state,
                    "fields": data["fields"], "error": data.get("error"),
                    "config_generation": data["config_generation"],
                    "progress": {"done": len(data["fields"]), "total": len(data["template"]["fields"])}}

    @app.post("/api/v1/jobs/{job_id}/cancel")
    def cancel_job(job_id: str, principal=Depends(auth)):
        with store.transaction() as session:
            job = get_job(session, job_id, principal)
            if job.state not in TERMINAL:
                job.state, job.fence = "cancelled", job.fence + 1
                store.event(session, job, "cancelled")
            return {"id": job.id, "state": job.state}

    @app.get("/api/v1/jobs/{job_id}/events")
    async def events(job_id: str, request: Request, after: int = 0, principal=Depends(auth)):
        with store.sessions() as session:
            get_job(session, job_id, principal)
        try:
            cursor = max(after, int(request.headers.get("last-event-id", "0")))
        except ValueError:
            error("invalid_event_cursor")
        if cursor < 0:
            error("invalid_event_cursor")

        async def stream():
            nonlocal cursor
            started = time.monotonic()
            while time.monotonic() - started < 60:
                if await request.is_disconnected():
                    break
                with store.sessions() as session:
                    job = session.scalar(select(Job).where(Job.id == job_id, Job.owner == principal["owner"]))
                    if not job:
                        break
                    rows = session.scalars(select(Event).where(Event.job_id == job_id, Event.owner == principal["owner"],
                                                               Event.seq > cursor).order_by(Event.seq).limit(100)).all()
                    terminal = job.state in TERMINAL
                    state = job.state
                for row in rows:
                    cursor = row.seq
                    yield f"id: {row.seq}\nevent: progress\ndata: {json.dumps({'job_id': job_id, 'kind': row.kind, 'state': state})}\n\n"
                if terminal and len(rows) < 100:
                    break
                yield ": heartbeat\n\n"
                await asyncio.sleep(.5)
        return StreamingResponse(stream(), media_type="text/event-stream",
                                 headers={"X-Accel-Buffering": "no"})

    @app.get("/api/v1/documents/{document_id}/results")
    def results(document_id: str, principal=Depends(auth)):
        with store.sessions() as session:
            return fields_result(session, get_document(session, document_id, principal))

    @app.patch("/api/v1/documents/{document_id}/fields/{field_id}")
    def correct(document_id: str, field_id: str, body: Correction, principal=Depends(auth)):
        with store.transaction() as session:
            document = get_document(session, document_id, principal)
            if document.version != body.revision:
                error("revision_conflict", 409)
            data = store.payload(document)
            spec = next((f for f in data["template"]["fields"] if f["field_id"] == field_id), None)
            if not spec:
                error("field_not_found", 404)
            if spec["type"] == "checkbox" and type(body.value) not in (bool, type(None)):
                error("checkbox_boolean_required")
            if spec["type"] != "checkbox" and type(body.value) not in (str, type(None)):
                error("string_value_required")
            review = {"value": body.value, "reviewer": principal["owner"], "at": time.time(),
                      "revision": document.version + 1}
            data["corrections"][field_id] = review
            data["review_state"] = "draft"
            store.record(session, principal["owner"], "review",
                         {"document_id": document_id, "field_id": field_id, **review})
            document.version += 1
            store.save(document, data)
            session.flush()
            return fields_result(session, document)

    @app.post("/api/v1/documents/{document_id}/review")
    def finalize(document_id: str, body: Revision, principal=Depends(auth)):
        with store.transaction() as session:
            document = get_document(session, document_id, principal)
            if document.version != body.revision:
                error("revision_conflict", 409)
            if session.scalar(select(Job.id).where(Job.document_id == document_id, Job.state.in_(("queued", "running")))):
                error("document_job_active", 409)
            result = fields_result(session, document)
            if any(not f["review"] or f["review"].get("stale") or f["validation_issues"]
                   for f in result["fields"].values()):
                error("human_review_or_validation_incomplete", 409)
            if result["amount_reconciliation"]["status"] in ("mismatch", "unparseable"):
                error("amount_reconciliation_failed", 409)
            data = store.payload(document)
            data["review_state"] = "reviewed"
            document.version += 1
            store.save(document, data)
            session.flush()
            return fields_result(session, document)

    @app.get("/api/v1/documents/{document_id}/export")
    def export(document_id: str, format: str = "json", mode: str = "draft", principal=Depends(auth)):
        if format not in ("json", "csv") or mode not in ("draft", "reviewed"):
            error("unsupported_export")
        with store.sessions() as session:
            result = fields_result(session, get_document(session, document_id, principal))
        if mode == "reviewed" and result["review_state"] != "reviewed":
            error("human_review_required", 409)
        if format == "json":
            return JSONResponse(result)
        output = io.StringIO()
        writer = csv.writer(output)
        writer.writerow(["field_id", "value", "review_state"])
        for fid, field in result["fields"].items():
            value = str(field["value"]) if field["value"] is not None else ""
            if value.lstrip().startswith(("=", "+", "-", "@", "\t", "\r", "\n")) or value.startswith(("\t", "\r", "\n")):
                value = "'" + value
            writer.writerow([fid, value, result["review_state"]])
        return Response(output.getvalue(), media_type="text/csv",
                        headers={"Content-Disposition": 'attachment; filename="review.csv"'})

    @app.get("/api/v1/models")
    def models(principal=Depends(auth)):
        return {"local_only": True, "items": [model_status(settings.model_dir)] + [
            {"model_id": name, "status": "unsupported_unverified", "inference_tested": False} for name in CANDIDATES],
            "providers": {"openrouter": "blocked_unsupported", "vllm": "blocked_unsupported"},
            "external_fallback": False}

    @app.get("/api/v1/settings/schema")
    def settings_schema(principal=Depends(admin)):
        return {"schema": Settings.model_json_schema(), "runtime_keys": sorted(HOT_KEYS),
                "help": {"secrets": "ICR_KEYS, ICR_ACTIVE_KEY, ICR_TOKENS are environment-only; never returned",
                         "providers": "No remote adapter or arbitrary endpoint is implemented"}}

    @app.get("/api/v1/settings")
    @app.get("/api/v1/settings/export")
    def get_settings(principal=Depends(admin)):
        with store.sessions() as session:
            config, revision, values = effective(session)
        return {"revision": revision, "values": config.model_dump(),
                "sources": {k: "runtime" if k in values and sources[k] not in ("environment", "explicit")
                            else sources[k] for k in Settings.model_fields},
                "runtime_keys": sorted(HOT_KEYS), "secrets": "[REDACTED]"}

    @app.patch("/api/v1/settings")
    @app.post("/api/v1/settings/import")
    def patch_settings(body: SettingsPatch, principal=Depends(admin)):
        if not body.values or set(body.values) - HOT_KEYS:
            error("restart_only_or_unknown_setting")
        if any(sources[k] in ("environment", "explicit") for k in body.values):
            error("startup_override_is_authoritative", 409)
        with store.transaction() as session:
            current, revision, values = effective(session)
            if revision != body.revision:
                error("revision_conflict", 409)
            try:
                Settings.model_validate({**current.model_dump(), **body.values})
            except ValueError:
                error("invalid_settings")
            row = session.get(Record, "__settings")
            if not row:
                row = store.record(session, "__system", "settings", {}, "__settings")
                row.version = 0
            store.save(row, {**values, **body.values})
            row.version += 1
            store.record(session, "__system", "settings_audit",
                         {"actor": principal["owner"], "keys": sorted(body.values), "revision": row.version})
            return {"revision": row.version, "applies_to": "new_jobs"}

    @app.post("/api/v1/admin/ledger/reset")
    def reset_ledger(principal=Depends(admin)):
        Ledger(settings.data_dir).reset(principal["owner"])
        return {"status": "reset_audited"}

    @app.post("/api/v1/admin/retention")
    def retention(principal=Depends(admin)):
        prune()
        return {"status": "retention_applied"}

    return app
