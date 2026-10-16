"""Studio-only integration tests; no existing backend suite is re-executed here."""
import os
import shutil
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from fastapi.testclient import TestClient

from backend.app.config import Settings
from backend.app.crypto import Keyring
from backend.app.main import create_app


class StudioServingTests(unittest.TestCase):
    def assert_api_boundary(self, compiled):
        Path("var").mkdir(exist_ok=True)
        with TemporaryDirectory() as root, TemporaryDirectory(dir="var") as data:
            if compiled:
                studio = Path(root) / "studio" / "dist"
                (studio / "assets").mkdir(parents=True)
                (studio / "index.html").write_text("<html>ICR Studio fixture</html>")
                (studio / "assets" / "app.js").write_text("export {};")
                (studio / "api" / "v1").mkdir(parents=True)
                (studio / "api" / "v1" / "static.txt").write_text("not an API response")
            with patch("backend.app.main.ROOT", Path(root)):
                app = create_app(
                    Settings(data_dir=str(Path(data).relative_to(Path.cwd())), worker_enabled=False),
                    Keyring({"test": os.urandom(32)}, "test"),
                    {"x" * 32: {"owner": "test", "admin": True}},
                )
            with TestClient(app) as client:
                for method in ("GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE", "CONNECT"):
                    for url in ("/api", "/api/", "/api/v1/missing", "/api/v1/providers/test",
                                "/api/v1/static.txt"):
                        with self.subTest(compiled=compiled, method=method, url=url):
                            response = client.request(method, url, json={"url": "http://169.254.169.254"})
                            self.assertEqual(response.status_code, 404)
                            self.assertEqual(response.headers["content-type"], "application/json")
                            self.assertEqual(response.json(), {
                                "code": "Not Found", "message": "Not Found",
                                "request_id": response.headers["x-request-id"], "details": [],
                            })
                            self.assertEqual(response.headers["cache-control"], "no-store")
                self.assertEqual(client.head("/api/v1/missing").status_code, 404)
                self.assertEqual(client.get("/api/v1/documents").status_code, 401)
                self.assertEqual(client.get("/api/v1/documents", headers={
                    "Authorization": "Bearer " + "x" * 32,
                }).status_code, 200)
                self.assertEqual(client.put("/api/v1/documents").status_code, 405)
                self.assertEqual(client.post("/healthz").status_code, 405)
                self.assertEqual(client.get("/healthz").json(), {"status": "alive"})
                self.assertEqual(client.get("/var/secrets.env").status_code, 404)
                response = client.get("/")
                self.assertEqual(response.status_code, 200 if compiled else 404)
                if compiled:
                    self.assertIn("ICR Studio fixture", response.text)
                    self.assertIn("frame-ancestors 'none'", response.headers["content-security-policy"])
                    self.assertEqual(client.get("/assets/app.js").text, "export {};")
                    self.assertEqual(client.post("/assets/app.js").status_code, 405)

    def test_api_boundary_with_compiled_studio(self):
        self.assert_api_boundary(compiled=True)

    def test_api_boundary_without_compiled_studio(self):
        self.assert_api_boundary(compiled=False)

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
