"""Unit/integration tests. Fake inference exists ONLY here; real missing-weight path tested too."""
import base64
import hashlib
import io
import json
import multiprocessing
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient
from PIL import Image
from sqlalchemy import select, update

from backend.app.config import Settings, load_settings
from backend.app.crypto import IntegrityError, Keyring, Sealed
from backend.app.db import Job, Record, Store
from backend.app.images import ImageRejected, canonical, crop
from backend.app.ledger import Ledger, fingerprint
from backend.app.main import create_app
from backend.app.ocr import MODEL_ID, ModelError, TrOCR, choose_device, verify_snapshot
from backend.app.schemas import Template
from backend.app.validation import amount_figures, amount_words, normalize, reconcile
from backend.app.worker import Worker

ROOT = Path(__file__).resolve().parents[1]
TEST_ROOT = ROOT / "var" / "test-backend"
TOKEN = "test-only-operator-token-not-a-real-secret"
OTHER = "test-only-other-user-token-not-a-real-secret"
USER = "test-only-non-admin-token-not-a-real-secret"
KEY_A = bytes(range(32))
KEY_B = bytes(range(32, 64))


def fake_inference(pipe, directory, device):
    """Clearly labeled protocol double; never imported by runtime."""
    while True:
        try:
            request = pipe.recv()
            if request is None:
                break
            pipe.send(("ok", {"raw_text": "Literal Test Name", "confidence": None,
                              "confidence_kind": "unavailable", "device": "cpu", "model_used": "TEST_DOUBLE"}))
        except EOFError:
            break


def slow_inference(pipe, directory, device):
    pipe.recv()
    time.sleep(60)


def ledger_process(directory, key):
    Ledger(directory).fail(key, "inference_failed")


def claim_process(directory, output):
    store = Store(Settings(data_dir=directory, worker_enabled=False), Keyring({"a": KEY_A}, "a"))
    output.put(store.claim(1))
    store.engine.dispose()


def image_bytes(size=(64, 32)):
    image = Image.new("RGB", size, "white")
    image.putpixel((3, 4), (13, 23, 33))
    output = io.BytesIO()
    image.save(output, format="PNG")
    return output.getvalue()


def tiny_template():
    return {"template_id": "test-lines", "template_version": "1", "form_kind": "GENERIC",
            "width_px": 64, "height_px": 32, "fields": [
                {"field_id": "name", "label": "Literal name", "type": "text", "required": True,
                 "roi": [0, 0, 64, 32], "line_rois": [[0, 0, 64, 32]]}]}


class Workspace(unittest.TestCase):
    def setUp(self):
        self.path = TEST_ROOT / self.__class__.__name__ / self._testMethodName
        self.assertFalse(self.path.exists())
        self.path.mkdir(parents=True, mode=0o700)
        self.relative = str(self.path.relative_to(ROOT))

    def tearDown(self):
        shutil.rmtree(self.path)


class CryptoTests(Workspace):
    def test_unique_nonce_aad_wrong_key_and_tamper(self):
        keys = Keyring({"a": KEY_A}, "a")
        first = keys.seal({"value": "Sensitive Name"}, "owner", "row", "records:data")
        second = keys.seal({"value": "Sensitive Name"}, "owner", "row", "records:data")
        self.assertNotEqual(first, second)
        self.assertNotIn(b"Sensitive", first.value)
        self.assertEqual(keys.open(first, "owner", "row", "records:data")["value"], "Sensitive Name")
        for owner, row, col in [("other", "row", "records:data"), ("owner", "other", "records:data"),
                                ("owner", "row", "jobs:data")]:
            with self.assertRaises(IntegrityError):
                keys.open(first, owner, row, col)
        with self.assertRaises(IntegrityError):
            Keyring({"a": KEY_B}, "a").open(first, "owner", "row", "records:data")
        data = json.loads(first.value)
        ciphertext = bytearray(base64.b64decode(data["ct"]))
        ciphertext[-1] ^= 1
        data["ct"] = base64.b64encode(ciphertext).decode()
        with self.assertRaises(IntegrityError):
            keys.open(Sealed(json.dumps(data).encode()), "owner", "row", "records:data")

    def test_orm_at_rest_bulk_guard_and_restartable_rotation(self):
        settings = Settings(data_dir=self.relative, worker_enabled=False)
        store = Store(settings, Keyring({"old": KEY_A}, "old"))
        with store.transaction() as session:
            rows = [store.record(session, "owner", "review", {"value": "Protected Test Person"}) for _ in range(4)]
            identifiers = [r.id for r in rows]
        with store.sessions() as session:
            self.assertEqual(store.payload(session.get(Record, identifiers[0]))["value"], "Protected Test Person")
        with self.assertRaises(Exception):
            with store.transaction() as session:
                session.execute(update(Record).values(data=b"PLAINTEXT BULK"))
        self.assertNotIn(b"Protected Test Person", (self.path / "icr.sqlite3").read_bytes())
        store.keys = Keyring({"old": KEY_A, "new": KEY_B}, "new")
        self.assertEqual(store.rotate(2), 2)
        with store.sessions() as session:
            for row in session.scalars(select(Record)):
                self.assertEqual(store.payload(row)["value"], "Protected Test Person")
        self.assertEqual(store.rotate(2), 2)
        self.assertEqual(store.rotate(2), 0)
        store.keys = Keyring({"new": KEY_B}, "new")
        with store.sessions() as session:
            self.assertTrue(all(store.payload(r) for r in session.scalars(select(Record))))
        store.engine.dispose()


class ConfigTests(Workspace):
    def test_precedence_strict_unknown_secrets(self):
        path = self.path / "config.yaml"
        path.write_text("max_tokens: 22\nattempt_seconds: 4\n")
        with patch.dict(os.environ, {"ICR_MAX_TOKENS": "33"}):
            settings, source = load_settings(path, {"attempt_seconds": 5})
        self.assertEqual(settings.max_tokens, 33)
        self.assertEqual(settings.attempt_seconds, 5)
        self.assertEqual(source["max_tokens"], "environment")
        self.assertEqual(source["attempt_seconds"], "explicit")
        for bad in [{"typo": True}, {"max_tokens": 0}, {"worker_enabled": "false"},
                    {"local_only": False}, {"dtype": "bfloat16"}, {"host": "0.0.0.0"},
                    {"api_key": "not-configurable"}, {"data_dir": "data"}, {"data_dir": "var/../data"},
                    {"cors_origins": ["https://example.test/path"]},
                    {"cors_origins": ["http://192.168.1.1:8000"]}]:
            with self.assertRaises(ValueError):
                Settings.model_validate(bad)
        for path in (ROOT / "config").glob("*.yaml"):
            self.assertEqual(load_settings(path)[0].schema_version, 1)


class ValidationTests(unittest.TestCase):
    def test_numeric_substitutions_only_explicit_box(self):
        spec = {"field_id": "account", "type": "comb", "required": True}
        self.assertEqual(normalize("OIlSB", spec)[0], "OIlSB")
        self.assertEqual(normalize("OIlSB", {**spec, "numeric": True})[0], "01158")
        self.assertEqual(normalize("00012", {**spec, "numeric": True})[0], "00012")
        self.assertIn("numeric_box_requires_digits", normalize("12-X", {**spec, "numeric": True})[2])

    def test_identifier_calendar_ambiguity_and_boolean(self):
        self.assertFalse(normalize("ABCD0123456", {"field_id": "ifsc", "type": "ifsc"})[2])
        self.assertTrue(normalize("ABCD1123456", {"field_id": "ifsc", "type": "ifsc"})[2])
        self.assertFalse(normalize("ABCDE1234F", {"field_id": "pan", "type": "pan"})[2])
        self.assertTrue(normalize("ABCDE123F", {"field_id": "pan", "type": "pan"})[2])
        spec = {"field_id": "date", "type": "date"}
        self.assertIn("invalid_calendar_date", normalize("30/02/2024", spec)[2])
        self.assertIn("ambiguous_date_order", normalize("03/04/2024", spec)[2])
        self.assertEqual(normalize("29/02/2024", spec)[0], "2024-02-29")
        self.assertEqual(normalize(False, {"field_id": "check", "type": "checkbox"})[0], False)

    def test_car_words_lar_figures_indian_decimals_and_unknown(self):
        self.assertEqual(amount_words("Rupees two lakh thirty four thousand five hundred and six only")[0],
                         amount_figures("2,34,506")[0])
        self.assertEqual(amount_words("one crore and fifty paise")[0], amount_figures("1,00,00,000.50")[0])
        self.assertEqual(amount_words("one point two five")[0], amount_figures("1.25")[0])
        for bad in ["one two", "and", "billion dollars", "one point twelve", ""]:
            self.assertIsNone(amount_words(bad)[0])
        for bad in ["1,23", "NaN", "-10", "1.123", "1e5"]:
            self.assertEqual(amount_figures(bad)[1], "unparseable")
        specs = [{"field_id": "amount_words", "type": "amount_words"},
                 {"field_id": "amount_figures", "type": "amount_figures"}]
        self.assertEqual(reconcile({}, specs)["status"], "unknown")
        self.assertEqual(reconcile({"amount_words": {"value": "ten"}, "amount_figures": {"value": "11"}}, specs)["status"], "mismatch")
        self.assertIsNone(reconcile({}, [{"field_id": "CAR", "type": "text"}])["match"])


class GeometryTests(unittest.TestCase):
    def test_pixel_crops_and_mime_limits(self):
        raw = image_bytes()
        canonical_raw, width, height = canonical(raw, "image/png", 10000, 150)
        self.assertEqual((width, height), (64, 32))
        with Image.open(io.BytesIO(crop(canonical_raw, [3, 4, 4, 5]))) as image:
            self.assertEqual(image.size, (1, 1))
            self.assertEqual(image.getpixel((0, 0)), (13, 23, 33))
        for box in [[-1, 0, 2, 2], [0, 0, 65, 20], [3, 4, 3, 4]]:
            with self.assertRaises(ImageRejected):
                crop(raw, box)
        with self.assertRaises(ImageRejected):
            canonical(raw, "image/jpeg", 10000, 150)
        with self.assertRaises(ImageRejected):
            canonical(raw, "image/png", 100, 150)

    def test_exif_orientation_and_metadata_strip(self):
        output = io.BytesIO()
        image = Image.new("RGB", (40, 20), "white")
        exif = Image.Exif()
        exif[274] = 6
        image.save(output, format="JPEG", exif=exif)
        raw, w, h = canonical(output.getvalue(), "image/jpeg", 10000, 150)
        self.assertEqual((w, h), (20, 40))
        with Image.open(io.BytesIO(raw)) as normalized:
            self.assertNotIn("exif", normalized.info)

    def test_pdf_bounded_pages_and_geometry_rejection(self):
        import pypdfium2 as pdfium
        pdf = pdfium.PdfDocument.new()
        pdf.new_page(72, 72)
        output = io.BytesIO()
        pdf.save(output)
        self.assertEqual(canonical(output.getvalue(), "application/pdf", 100000, 150)[1:], (150, 150))
        pdf.new_page(72, 72)
        output = io.BytesIO()
        pdf.save(output)
        with self.assertRaises(ImageRejected):
            canonical(output.getvalue(), "application/pdf", 100000, 150)
        pdf.close()
        template = tiny_template()
        template["fields"][0]["roi"] = [0, 0, 100, 100]
        with self.assertRaises(ValueError):
            Template.model_validate(template)
        template["fields"][0]["roi"] = [0, 0, 64, 32]
        template["fields"][0]["cells"] = [[0, 0, 4, 4]]
        with self.assertRaises(ValueError):
            Template.model_validate(template)


class LedgerTests(Workspace):
    def test_multiprocess_atomic_failure_skip_and_reset_audit(self):
        context = multiprocessing.get_context("spawn")
        key = fingerprint({"device": "cpu", "dtype": "float32", "image": "protected_hmac"})
        processes = [context.Process(target=ledger_process, args=(self.relative, key)) for _ in range(4)]
        for process in processes:
            process.start()
        for process in processes:
            process.join(15)
            self.assertEqual(process.exitcode, 0)
        ledger = Ledger(self.relative)
        self.assertTrue(ledger.blocked(key))
        self.assertEqual(json.loads(ledger.path.read_text())["issues"][key]["count"], 4)
        self.assertNotEqual(key, fingerprint({"device": "cuda", "dtype": "float32", "image": "protected_hmac"}))
        ledger.reset("admin")
        self.assertFalse(ledger.blocked(key))
        self.assertEqual(len(json.loads(ledger.path.read_text())["resets"]), 1)

    def test_two_process_claim_and_stale_fence(self):
        settings = Settings(data_dir=self.relative, worker_enabled=False)
        store = Store(settings, Keyring({"a": KEY_A}, "a"))
        with store.transaction() as session:
            job = Job(id="job", owner="owner", document_id="doc", idempotency="claim-key", digest="digest",
                      state="queued", fence=0, lease=0)
            store.save(job, {"fields": {}})
            session.add(job)
        context = multiprocessing.get_context("spawn")
        output = context.Queue()
        processes = [context.Process(target=claim_process, args=(self.relative, output)) for _ in range(2)]
        for process in processes:
            process.start()
        for process in processes:
            process.join(15)
            self.assertEqual(process.exitcode, 0)
        claims = [output.get(timeout=2) for _ in range(2)]
        self.assertEqual(sum(c is not None for c in claims), 1)
        with store.transaction() as session:
            session.get(Job, "job").lease = time.time() - 1
        self.assertIsNone(store.claim(1))
        self.assertFalse(Worker(store, settings).update("job", 1, {"fields": {}}, "needs_review"))
        with store.sessions() as session:
            self.assertEqual(session.get(Job, "job").state, "failed")
        store.engine.dispose()
        output.close()


class ModelTests(Workspace):
    def test_missing_weights_never_imports_heavy_libs(self):
        with patch("builtins.__import__", wraps=__import__) as imports:
            with self.assertRaises(ModelError) as result:
                TrOCR(self.path / "absent", "cpu").load()
            self.assertEqual(result.exception.code, "model_unavailable")
            self.assertFalse(any(c.args[0] in ("torch", "transformers") for c in imports.call_args_list))

    def test_offline_verifier_rejects_unapproved_corrupt_extra_files(self):
        config = self.path / "config.json"
        config.write_text('{"model_type":"vision-encoder-decoder"}')
        weights = self.path / "model.safetensors"
        weights.write_bytes(b"TEST ONLY not real weights")
        card = self.path / "README.md"
        card.write_text("---\nlicense: TEST_ONLY\n---\nSynthetic verifier unit test, NOT weights.\n")
        manifest = {"schema_version": 1, "model_id": MODEL_ID, "revision": "a" * 40, "license": "TEST_ONLY",
                    "license_evidence": f"https://huggingface.co/{MODEL_ID}/blob/{'a' * 40}/README.md",
                    "operator_approved": True, "files": {
                        p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (config, weights, card)}}
        (self.path / "manifest.json").write_text(json.dumps(manifest))
        self.assertEqual(verify_snapshot(self.path)["revision"], "a" * 40)
        weights.write_bytes(b"tampered")
        with self.assertRaises(ModelError):
            verify_snapshot(self.path)
        weights.write_bytes(b"TEST ONLY not real weights")
        for changes in ({"operator_approved": False}, {"operator_approved": "false"},
                        {"model_id": "Riksarkivet/trocr-base-handwritten"},
                        {"license": "OTHER"}, {"revision": "main"}):
            (self.path / "manifest.json").write_text(json.dumps({**manifest, **changes}))
            with self.assertRaises(ModelError):
                verify_snapshot(self.path)
        (self.path / "manifest.json").write_text(json.dumps(manifest))
        (self.path / "remote.py").write_text("never execute")
        with self.assertRaises(ModelError):
            verify_snapshot(self.path)

    def test_benchmark_missing_weights_and_download_no_opt_in(self):
        image = self.path / "line.png"
        image.write_bytes(image_bytes())
        manifest = self.path / "evaluation.json"
        manifest.write_text(json.dumps([{"image": str(image), "reference": "TEST ONLY evaluation reference"}]))
        result = subprocess.run([sys.executable, "scripts/benchmark", "--manifest", str(manifest),
                                 "--model-dir", str(self.path / "missing")], capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(json.loads(result.stdout)["measurements"], None)
        self.assertEqual(json.loads(result.stdout)["status"], "model_unavailable")
        result = subprocess.run([sys.executable, "scripts/download_models"], capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"--download and explicit --approve-license", result.stderr)


class ApiTests(Workspace):
    def setUp(self):
        super().setUp()
        self.settings = Settings(data_dir=self.relative, worker_enabled=False,
                                 model_dir=self.relative + "/missing", attempt_seconds=1, job_seconds=3)
        self.app = create_app(self.settings, Keyring({"a": KEY_A}, "a"), {
            TOKEN: {"owner": "operator", "admin": True},
            OTHER: {"owner": "other", "admin": True},
            USER: {"owner": "operator", "admin": False}})
        self.client_context = TestClient(self.app)
        self.client = self.client_context.__enter__()
        self.client.headers["Authorization"] = "Bearer " + TOKEN

    def tearDown(self):
        self.client_context.__exit__(None, None, None)
        super().tearDown()

    def document(self):
        response = self.client.post("/api/v1/templates", json=tiny_template())
        self.assertEqual(response.status_code, 201, response.text)
        response = self.client.post("/api/v1/documents?template_id=test-lines&template_version=1",
                                    content=image_bytes(), headers={"Content-Type": "image/png"})
        self.assertEqual(response.status_code, 201, response.text)
        return response.json()["id"]

    def queue(self, document, key="test-job-key"):
        response = self.client.post(f"/api/v1/documents/{document}/jobs",
                                    json={"idempotency_key": key, "alignment_confirmed": True})
        self.assertEqual(response.status_code, 202, response.text)
        return response.json()["id"]

    def test_auth_owner_scopes_and_provider_block(self):
        self.assertEqual(self.client.get("/api/v1/documents", headers={"Authorization": ""}).status_code, 401)
        document = self.document()
        self.assertEqual(self.client.get(f"/api/v1/documents/{document}", headers={"Authorization": "Bearer " + OTHER}).status_code, 404)
        self.assertEqual(self.client.get("/api/v1/settings", headers={"Authorization": "Bearer " + USER}).status_code, 403)
        self.assertEqual(self.client.post(f"/api/v1/documents/{document}/jobs",
                         json={"idempotency_key": "test-remote", "alignment_confirmed": True, "remote_consent": True}).status_code, 422)
        self.assertEqual(self.client.post("/api/v1/providers/test", json={"url": "http://169.254.169.254"}).status_code, 404)
        models = self.client.get("/api/v1/models").json()
        self.assertFalse(models["external_fallback"])
        self.assertEqual(models["providers"]["openrouter"], "blocked_unsupported")
        self.assertEqual(self.client.get("/healthz").json()["status"], "alive")
        self.assertEqual(self.client.get("/readyz").json()["inference"], "model_unavailable")

    def test_real_missing_weight_job_ledger_events_cancel_rerun(self):
        document = self.document()
        job_id = self.queue(document)
        store = self.app.state.store
        self.app.state.worker.run(*store.claim(3))
        job = self.client.get(f"/api/v1/jobs/{job_id}").json()
        self.assertEqual(job["state"], "failed")
        field = job["fields"]["name"]
        self.assertEqual(field["issues"], ["model_unavailable"])
        self.assertIsNone(field["raw_text"])
        self.assertIsNone(field["confidence"])
        events = self.client.get(f"/api/v1/jobs/{job_id}/events").text
        self.assertIn("event: progress", events)
        self.assertIn('"kind": "failed"', events)
        second = self.queue(document, "second-job-key")
        self.app.state.worker.run(*store.claim(3))
        self.assertEqual(self.client.get(f"/api/v1/jobs/{second}").json()["fields"]["name"]["issues"], ["quarantined_failure"])
        third = self.queue(document, "third-job-key")
        self.assertEqual(self.client.post(f"/api/v1/jobs/{third}/cancel").json()["state"], "cancelled")
        self.assertEqual(self.client.post("/api/v1/admin/ledger/reset").status_code, 200)

    def test_test_only_double_raw_immutable_review_revision_and_export(self):
        document = self.document()
        job_id = self.queue(document)
        worker = Worker(self.app.state.store, self.settings, fake_inference)
        worker.run(*self.app.state.store.claim(3))
        self.assertEqual(self.client.get(f"/api/v1/jobs/{job_id}").json()["state"], "needs_review")
        result = self.client.get(f"/api/v1/documents/{document}/results").json()
        self.assertEqual(result["fields"]["name"]["raw_text"], "Literal Test Name")
        response = self.client.patch(f"/api/v1/documents/{document}/fields/name",
                                     json={"revision": result["revision"], "value": "=Formula Safe Name"})
        self.assertEqual(response.status_code, 200, response.text)
        edited = response.json()
        self.assertEqual(edited["fields"]["name"]["raw_text"], "Literal Test Name")
        self.assertEqual(edited["fields"]["name"]["value"], "=Formula Safe Name")
        self.assertEqual(self.client.patch(f"/api/v1/documents/{document}/fields/name",
                         json={"revision": result["revision"], "value": "stale"}).status_code, 409)
        self.assertEqual(self.client.get(f"/api/v1/documents/{document}/export?mode=reviewed").status_code, 409)
        self.assertIn("'=Formula Safe Name", self.client.get(f"/api/v1/documents/{document}/export?format=csv").text)
        finalized = self.client.post(f"/api/v1/documents/{document}/review", json={"revision": edited["revision"]})
        self.assertEqual(finalized.status_code, 200, finalized.text)
        self.assertEqual(self.client.get(f"/api/v1/documents/{document}/export?mode=reviewed").status_code, 200)
        rerun = self.queue(document, "review-invalidating-rerun")
        self.client.post(f"/api/v1/jobs/{rerun}/cancel")
        stale = self.client.get(f"/api/v1/documents/{document}/results").json()
        self.assertTrue(stale["fields"]["name"]["review"]["stale"])
        self.assertEqual(self.client.post(f"/api/v1/documents/{document}/review",
                         json={"revision": stale["revision"]}).status_code, 409)
        for path in self.path.glob("icr.sqlite3*"):
            self.assertNotIn(b"Formula Safe Name", path.read_bytes())
            self.assertNotIn(b"Literal Test Name", path.read_bytes())
        self.assertEqual(self.client.delete(f"/api/v1/documents/{document}").status_code, 204)
        self.assertEqual(self.client.get(f"/api/v1/jobs/{job_id}").status_code, 404)

    def test_upload_constraints_samples_geometry_and_unsupported_ops(self):
        document = self.document()
        self.assertEqual(self.client.get(f"/api/v1/documents/{document}/image").headers["cache-control"], "no-store")
        self.assertEqual(self.client.get(f"/api/v1/documents/{document}/crops/name").status_code, 200)
        response = self.client.post("/api/v1/documents?template_id=test-lines&template_version=1",
                                    content=b"not a png", headers={"Content-Type": "image/png"})
        self.assertEqual(response.status_code, 422)
        self.assertEqual(self.client.post("/api/v1/documents?template_id=test-lines&template_version=1",
                         content=image_bytes(), headers={"Content-Type": "text/plain"}).status_code, 415)
        self.assertEqual(self.client.post(f"/api/v1/documents/{document}/jobs",
                         json={"idempotency_key": "unsupported-key", "alignment_confirmed": True,
                               "deskew": True}).status_code, 422)
        self.assertEqual(len(self.client.get("/api/v1/samples").json()["items"]), 6)
        self.assertEqual(self.client.post("/api/v1/samples/01-car/import").status_code, 201)
        self.assertEqual(self.client.post("/api/v1/samples/06-car-noisy-invalid/import").status_code, 404)
        samples = self.client.get("/api/v1/samples").json()["items"]
        noisy = next(s for s in samples if s["alignment_required"])
        self.assertEqual(self.client.post(f"/api/v1/samples/{noisy['sample_id']}/import").status_code, 422)
        self.assertEqual(self.client.post("/api/v1/templates", content=b" " * 262145,
                         headers={"Content-Type": "application/json"}).status_code, 413)
        self.assertEqual(self.client.post("/api/v1/documents?template_id=test-lines&template_version=1",
                         content=b"x" * (10485760 + 1), headers={"Content-Type": "image/png"}).status_code, 413)

    def test_idempotency_owner_scoped_and_immutable_templates(self):
        document = self.document()
        self.assertEqual(self.client.post("/api/v1/templates", json=tiny_template()).status_code, 409)
        job = self.queue(document)
        self.assertEqual(self.queue(document), job)
        self.client.post(f"/api/v1/jobs/{job}/cancel")
        other_document = self.client.post("/api/v1/samples/01-car/import").json()["id"]
        self.assertEqual(self.client.post(f"/api/v1/documents/{other_document}/jobs",
                         json={"idempotency_key": "test-job-key", "alignment_confirmed": True}).status_code, 409)
        self.assertEqual(self.client.get(f"/api/v1/jobs/{job}/events",
                         headers={"Authorization": "Bearer " + OTHER}).status_code, 404)
        self.assertEqual(self.client.get(f"/api/v1/documents/{document}/image",
                         headers={"Authorization": "Bearer " + OTHER}).status_code, 404)

    def test_settings_hot_import_schema_redaction_and_restart_rejection(self):
        settings = self.client.get("/api/v1/settings").json()
        self.assertEqual(settings["secrets"], "[REDACTED]")
        self.assertNotIn(TOKEN, json.dumps(settings))
        self.assertIn("max_tokens", self.client.get("/api/v1/settings/schema").json()["runtime_keys"])
        response = self.client.patch("/api/v1/settings", json={"revision": 0, "values": {"max_tokens": 16}})
        self.assertEqual(response.status_code, 200, response.text)
        self.assertEqual(self.client.get("/api/v1/settings/export").json()["values"]["max_tokens"], 16)
        self.assertEqual(self.client.patch("/api/v1/settings", json={"revision": 0, "values": {"max_tokens": 18}}).status_code, 409)
        self.assertEqual(self.client.post("/api/v1/settings/import", json={"revision": 1, "values": {"model_dir": "evil"}}).status_code, 422)
        self.assertEqual(self.client.patch("/api/v1/settings", json={"revision": 1, "values": {"max_tokens": 1000}}).status_code, 422)

    def test_deadline_and_running_cancel_fenced(self):
        document = self.document()
        job_id = self.queue(document)
        worker = Worker(self.app.state.store, self.settings, slow_inference)
        claim = self.app.state.store.claim(3)
        start = time.monotonic()
        worker.run(*claim)
        self.assertLess(time.monotonic() - start, 8)
        self.assertEqual(self.client.get(f"/api/v1/jobs/{job_id}").json()["error"], "deadline_exceeded")
        job_id = self.queue(document, "cancel-running-key")
        # Different fingerprint via new token budget avoids the prior per-crop quarantine.
        with self.app.state.store.transaction() as session:
            job = session.get(Job, job_id)
            data = self.app.state.store.payload(job)
            data["settings"]["max_tokens"] = 64
            self.app.state.store.save(job, data)
        claim = self.app.state.store.claim(3)
        thread = threading.Thread(target=worker.run, args=claim)
        thread.start()
        time.sleep(.25)
        self.client.post(f"/api/v1/jobs/{job_id}/cancel")
        thread.join(5)
        self.assertFalse(thread.is_alive())
        self.assertEqual(self.client.get(f"/api/v1/jobs/{job_id}").json()["state"], "cancelled")

    def test_retention_removes_expired_encrypted_document_and_jobs(self):
        document = self.document()
        job = self.queue(document)
        with self.app.state.store.transaction() as session:
            session.get(Record, document).created = time.time() - 8 * 86400
        self.assertEqual(self.client.post("/api/v1/admin/retention").status_code, 200)
        self.assertEqual(self.client.get(f"/api/v1/documents/{document}").status_code, 404)
        self.assertEqual(self.client.get(f"/api/v1/jobs/{job}").status_code, 404)


if __name__ == "__main__":
    unittest.main()
