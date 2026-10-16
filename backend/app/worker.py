"""One durable inference slot; each owned subprocess is killable and fenced."""
import base64
import hashlib
import hmac
import importlib.metadata
import json
import multiprocessing
import threading
import time
from pathlib import Path

from sqlalchemy import select

from .config import Settings
from .db import Job, Record
from .images import crop
from .ledger import Ledger, fingerprint
from .ocr import MODEL_ID, inference_child
from .validation import normalize

TERMINAL = {"completed", "needs_review", "failed", "cancelled", "partial"}


def runtime_fingerprint(settings):
    libraries = {}
    for name in ("torch", "transformers", "Pillow", "pypdfium2"):
        try:
            libraries[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            libraries[name] = "absent"
    directory = Path(settings.model_dir)
    manifest = directory / "manifest.json"
    return {"adapter": "trocr-v1", "model": MODEL_ID,
            "artifact": hashlib.sha256(manifest.read_bytes()).hexdigest() if manifest.exists() and manifest.stat().st_size <= 1048576 else "missing_or_invalid",
            "device": settings.device, "dtype": settings.dtype, "attention": "eager", "quantization": "none",
            "libraries": libraries, "config": fingerprint(settings.model_dump()),
            "lock": hashlib.sha256(Path("uv.lock").read_bytes()).hexdigest() if Path("uv.lock").exists() else "absent"}


class Worker:
    def __init__(self, store, settings, child_target=inference_child):
        self.store, self.settings = store, settings
        self.ledger = Ledger(settings.data_dir)
        self.target = child_target
        self.stop = threading.Event()
        self.thread = None

    def start(self):
        self.thread = threading.Thread(target=self.loop, daemon=True, name="icr-queue")
        self.thread.start()

    def close(self):
        self.stop.set()
        if self.thread:
            self.thread.join(8)

    def loop(self):
        while not self.stop.is_set():
            try:
                claim = self.store.claim(1800)
                if claim:
                    self.run(*claim)
                else:
                    self.stop.wait(.25)
            except Exception:
                # No PII, file paths, or exception strings enter logs.
                self.stop.wait(1)

    def update(self, job_id, fence, payload, state=None, event="field"):
        with self.store.transaction() as session:
            job = session.get(Job, job_id)
            if not job or job.fence != fence or job.state != "running":
                return False
            if state:
                job.state = state
            self.store.save(job, payload)
            self.store.event(session, job, event)
            return True

    def active(self, job_id, fence):
        if self.stop.is_set():
            return False
        with self.store.sessions() as session:
            job = session.get(Job, job_id)
            return bool(job and job.state == "running" and job.fence == fence)

    def run(self, job_id, fence):
        with self.store.sessions() as session:
            job = session.get(Job, job_id)
            payload = self.store.payload(job)
            owner = job.owner
            document = session.scalar(select(Record).where(Record.id == job.document_id,
                                                           Record.owner == owner, Record.kind == "document"))
            if not document:
                payload["error"] = "document_missing"
                self.update(job_id, fence, payload, "failed", "failed")
                return
            image = base64.b64decode(self.store.payload(document)["image"])
        settings = Settings.model_validate(payload["settings"])
        runtime = runtime_fingerprint(settings)
        global_key = fingerprint({"stage": "load", **runtime})
        context = multiprocessing.get_context("spawn")
        parent, child = context.Pipe()
        process = None
        start = time.monotonic()
        failures, successes = 0, 0
        partial_evidence = False
        try:
            for spec in payload["template"]["fields"]:
                if not self.active(job_id, fence):
                    return
                result = {"field_id": spec["field_id"], "raw_text": None, "normalized_value": None,
                          "confidence": None, "confidence_kind": "unavailable", "issues": [],
                          "status": "needs_review", "model_used": MODEL_ID,
                          "bbox": spec["roi"], "line_rois": spec["line_rois"], "provenance": {
                              "runtime": runtime, "attempts": [], "human_review_required": True}}
                raw_lines = []
                truncated = False
                if spec["type"] == "checkbox":
                    result["issues"] = ["manual_checkbox_required"]
                else:
                    for box in spec.get("cells") or spec["line_rois"] or [spec["roi"]]:
                        if not self.active(job_id, fence):
                            return
                        raw = crop(image, box)
                        digest = hmac.new(self.store.keys.keys[self.store.keys.active],
                                          owner.encode() + b":crop:" + raw, hashlib.sha256).hexdigest()
                        key = fingerprint({"stage": "inference", **runtime, "image_hmac": digest})
                        attempt = {"fingerprint": key, "crop_hmac": digest, "bbox": box}
                        result["provenance"]["attempts"].append(attempt)
                        code = None
                        if self.ledger.blocked(global_key) or self.ledger.blocked(key):
                            code = "quarantined_failure"
                        elif time.monotonic() - start >= settings.job_seconds:
                            code = "job_deadline_exceeded"
                        else:
                            if process is None:
                                process = context.Process(target=self.target,
                                                          args=(child, settings.model_dir, settings.device))
                                process.start()
                                child.close()
                            attempt_start = time.monotonic()
                            parent.send((raw, settings.max_tokens))
                            limit = min(settings.attempt_seconds,
                                        settings.job_seconds - (time.monotonic() - start))
                            while not parent.poll(.1):
                                if not self.active(job_id, fence):
                                    return
                                if time.monotonic() - attempt_start >= limit:
                                    code = "deadline_exceeded"
                                    break
                                if not process.is_alive():
                                    code = "inference_failed"
                                    break
                            if code is None:
                                status, output = parent.recv()
                                if status == "ok":
                                    raw_lines.append(output["raw_text"])
                                    truncated |= output.get("truncated", False)
                                    result.update({k: output[k] for k in ("model_used", "confidence", "confidence_kind")})
                                    result["provenance"]["device"] = output["device"]
                                else:
                                    code = output
                            attempt["seconds"] = round(time.monotonic() - attempt_start, 4)
                            if code:
                                self.ledger.fail(global_key if code in {
                                    "model_unavailable", "artifact_invalid", "dependency_unavailable",
                                    "device_unavailable"} else key, code)
                        attempt["outcome"] = code or "success"
                        if code:
                            result["issues"].append(code)
                            break
                    if not result["issues"]:
                        result["raw_text"] = ("" if spec.get("cells") else "\n").join(raw_lines)
                        result["normalized_value"], result["transformations"], result["issues"] = normalize(
                            result["raw_text"], spec)
                        if truncated:
                            result["issues"].append("generation_truncated")
                        result["status"] = "recognized" if not result["issues"] else "needs_review"
                        successes += 1
                    else:
                        # Partial line evidence is retained separately, not invented as a complete value.
                        result["partial_raw_lines"] = raw_lines
                        partial_evidence |= bool(raw_lines)
                        result["status"] = "unavailable"
                        failures += 1
                payload["fields"][spec["field_id"]] = result
                if not self.update(job_id, fence, payload):
                    return
                if "deadline_exceeded" in result["issues"]:
                    payload["error"] = "deadline_exceeded"
                    break
            payload["elapsed_seconds"] = round(time.monotonic() - start, 4)
            state = "partial" if failures and (successes or partial_evidence) else "failed" if failures else "needs_review"
            self.update(job_id, fence, payload, state, state)
        except Exception:
            payload["error"] = "inference_failed"
            self.update(job_id, fence, payload, "partial" if successes else "failed", "failed")
        finally:
            if process:
                if process.is_alive():
                    process.terminate()
                process.join(3)
                if process.is_alive():
                    process.kill()
                    process.join()
            parent.close()
            child.close()
