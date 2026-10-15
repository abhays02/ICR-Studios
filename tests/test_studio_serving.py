"""Studio-only integration tests; no existing backend suite is re-executed here."""
import os
import shutil
import unittest
from pathlib import Path

from fastapi.testclient import TestClient

from backend.app.config import Settings
from backend.app.crypto import Keyring
from backend.app.main import create_app


class StudioServingTests(unittest.TestCase):
    def test_compiled_studio_is_same_origin_and_private_files_are_not_served(self):
        if not Path("studio/dist/index.html").exists():
            self.skipTest("Build Studio with npm ci && npm run build first")
        path = Path("var/test-studio-static")
        path.mkdir(parents=True, exist_ok=True)
        try:
            keys = Keyring({"test": os.urandom(32)}, "test")
            app = create_app(Settings(data_dir=str(path), worker_enabled=False), keys,
                             {"x" * 32: {"owner": "test", "admin": True}})
            with TestClient(app) as client:
                response = client.get("/")
                self.assertEqual(response.status_code, 200)
                self.assertIn("ICR Studio", response.text)
                self.assertIn("frame-ancestors 'none'", response.headers["content-security-policy"])
                self.assertEqual(client.get("/var/secrets.env").status_code, 404)
                self.assertEqual(client.get("/api/v1/documents").status_code, 401)
                self.assertEqual(client.get("/healthz").json(), {"status": "alive"})
        finally:
            shutil.rmtree(path)
