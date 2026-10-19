"""Packaging contracts, runnable with the repository's existing unittest runner."""
import base64
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest
from unittest.mock import MagicMock, patch
import uuid

import yaml


ROOT = Path(__file__).resolve().parents[1]


def load_module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "packaging" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


entry = load_module("windows_entry")
builder = load_module("build_windows")
smoke = load_module("smoke_windows")


class WindowsPackagingTests(unittest.TestCase):
    def setUp(self):
        self.path = ROOT / "var" / "windows-unit" / uuid.uuid4().hex
        self.path.mkdir(parents=True)

    def tearDown(self):
        shutil.rmtree(self.path)

    def test_secret_generation_is_exclusive_and_environment_format_is_valid(self):
        path = self.path / "var/windows-secrets.json"
        entry.initialize_secrets(path)
        raw = path.read_bytes()
        values = json.loads(raw)
        keys = json.loads(values["ICR_KEYS"])
        self.assertEqual(len(base64.b64decode(keys[values["ICR_ACTIVE_KEY"]], validate=True)), 32)
        principals = json.loads(values["ICR_TOKENS"])
        self.assertEqual(len(principals), 1)
        token = next(iter(principals))
        self.assertGreaterEqual(len(token), 32)
        self.assertEqual(principals[token], {"owner": "operator", "admin": True})
        with self.assertRaises(FileExistsError):
            entry.initialize_secrets(path)
        self.assertEqual(path.read_bytes(), raw)
        with patch.dict(os.environ, {}, clear=True):
            entry.load_secrets(path)
            for name in entry.SECRET_NAMES:
                self.assertEqual(os.environ[name], values[name])

    def test_environment_credentials_are_all_or_none(self):
        path = self.path / "must-not-be-read.json"
        with patch.dict(os.environ, {"ICR_KEYS": "{}"}, clear=True):
            with self.assertRaises(ValueError):
                entry.load_secrets(path)
        supplied = {name: "externally-injected" for name in entry.SECRET_NAMES}
        with patch.dict(os.environ, supplied, clear=True):
            entry.load_secrets(path)
            self.assertEqual({name: os.environ[name] for name in entry.SECRET_NAMES}, supplied)

    def test_invalid_secret_file_is_rejected_without_environment_mutation(self):
        path = self.path / "invalid.json"
        for values in ({"ICR_KEYS": "x"}, {name: 42 for name in entry.SECRET_NAMES}):
            path.write_text(json.dumps(values), encoding="utf-8")
            with patch.dict(os.environ, {}, clear=True):
                with self.assertRaises(ValueError):
                    entry.load_secrets(path)
                self.assertFalse(any(name in os.environ for name in entry.SECRET_NAMES))

    def test_source_build_cannot_cross_compile_a_claimed_windows_executable(self):
        with patch.object(builder.sys, "platform", "linux"):
            with self.assertRaisesRegex(SystemExit, "cross-compilation"):
                builder.main()

    def test_pr_and_branch_builds_have_no_release_write_permissions(self):
        workflow = yaml.load((ROOT / ".github/workflows/windows-release.yml").read_text(),
                             Loader=yaml.BaseLoader)
        self.assertIn("pull_request", workflow["on"])
        self.assertNotIn("pull_request_target", workflow["on"])
        self.assertEqual(workflow["on"]["push"]["branches"], ["**"])
        for dependency in ("backend/**", "studio/**", "packaging/**", "pyproject.toml", "uv.lock"):
            self.assertIn(dependency, workflow["on"]["push"]["paths"])
        self.assertEqual(workflow["permissions"], {"contents": "read"})
        self.assertNotIn("permissions", workflow["jobs"]["build"])
        release = workflow["jobs"]["release"]
        self.assertEqual(release["permissions"], {"contents": "write"})
        self.assertIn("needs.build.outputs.release_tag != ''", release["if"])
        self.assertIn("github.event_name == 'push' || inputs.publish", release["if"])
        self.assertEqual(release["needs"], "build")
        upload = next(step for step in workflow["jobs"]["build"]["steps"]
                      if step.get("uses", "").startswith("actions/upload-artifact@"))
        self.assertNotIn("if", upload)
        self.assertEqual(upload["with"]["path"].splitlines(),
                         ["var/release/*.zip", "var/release/*.sha256"])

    def test_resource_allowlist_contains_only_runtime_resources(self):
        files = builder.resource_files()
        self.assertIn((ROOT / "packaging/windows.yaml", "config"), files)
        sources = [path.relative_to(ROOT).as_posix() for path, _ in files]
        self.assertIn("data/templates/car-v1.0.0.json", sources)
        self.assertIn("data/schemas/car-v1.0.0.schema.json", sources)
        self.assertIn("data/samples/manifest.json", sources)
        self.assertIn("data/samples/01-car.png", sources)
        self.assertIn("data/samples/LICENSE-CC0.txt", sources)
        for source in sources:
            self.assertNotIn("web_demo", source)
            self.assertNotIn("real_handwriting", source)
            self.assertNotIn("secrets", source)
            self.assertNotIn("var/", source)
        for sample in json.loads((ROOT / "data/samples/manifest.json").read_text())["samples"]:
            self.assertIn("data/samples/" + sample["image"], sources)
            self.assertIn((ROOT / "data/samples" / sample["template"]).resolve(),
                          [path.resolve() for path, _ in files])

    def test_launcher_separates_resources_from_private_working_directory(self):
        resources = self.path / "resources"
        (resources / "config").mkdir(parents=True)
        (resources / "config/windows.yaml").write_text("schema_version: 1\n", encoding="utf-8")
        workspace = self.path / "workspace"
        workspace.mkdir()
        entry.initialize_secrets(workspace / "var/windows-secrets.json")
        original_cwd = Path.cwd()
        try:
            with (patch.object(entry, "RESOURCE_ROOT", resources),
                  patch.object(entry, "protect_workspace", return_value=workspace),
                  patch.dict(os.environ, {"LOCALAPPDATA": str(self.path), "ICR_HOST": "0.0.0.0",
                                          "ICR_DATA_DIR": "var/unwanted"}, clear=True),
                  patch("backend.app.main.create_app", return_value="application") as create_app,
                  patch("uvicorn.run") as run):
                entry.main(["--workspace", str(workspace), "--port", "8123", "serve"])
                settings = create_app.call_args.kwargs["settings"]
                self.assertEqual(settings.host, "127.0.0.1")
                self.assertEqual(settings.port, 8123)
                self.assertEqual(settings.data_dir, "var")
                self.assertEqual(settings.model_dir, "var/models-not-bundled")
                self.assertEqual(settings.cors_origins, [])
                self.assertEqual(Path.cwd(), workspace)
                self.assertEqual(run.call_args.args, ("application",))
                self.assertEqual(run.call_args.kwargs["loop"], "asyncio")
                self.assertFalse(run.call_args.kwargs["access_log"])
        finally:
            os.chdir(original_cwd)

    def test_launcher_rejects_resource_workspace_before_changing_acls(self):
        with (patch.object(entry, "RESOURCE_ROOT", self.path),
              patch.object(entry, "protect_workspace") as protect,
              patch.dict(os.environ, {"LOCALAPPDATA": str(self.path)})):
            with self.assertRaisesRegex(ValueError, "outside bundled"):
                entry.main(["--workspace", str(self.path / "private"), "init-secrets"])
            protect.assert_not_called()

    @unittest.skipIf(os.name == "nt", "POSIX-only fail-closed check")
    def test_launcher_refuses_non_windows_acl_fallback(self):
        with self.assertRaisesRegex(RuntimeError, "Windows"):
            entry.protect_workspace(self.path)

    def test_reparse_ancestors_are_rejected(self):
        target = self.path / "target"
        target.mkdir()
        link = self.path / "link"
        try:
            link.symlink_to(target, target_is_directory=True)
        except OSError:
            self.skipTest("Creating Windows symlinks is not permitted for this account")
        with self.assertRaisesRegex(ValueError, "reparse"):
            entry.reject_reparse(link / "future")


class WindowsSmokeTests(unittest.TestCase):
    def test_acl_check_resets_inherited_powershell_module_paths(self):
        workspace = ROOT / "var" / "windows-smoke" / "private-workspace"
        for key in ("PSModulePath", "PSMODULEPATH", "psmodulepath"):
            with self.subTest(key=key):
                env = {key: "PowerShell-7-modules", "PATH": "system-path"}
                original = env.copy()
                with patch.object(smoke.subprocess, "run") as run:
                    smoke.verify_workspace_acl(workspace, env)
                self.assertEqual(env, original)
                self.assertEqual(run.call_args.kwargs["env"],
                                 {"PATH": "system-path", "ICR_SMOKE_WORKSPACE": str(workspace)})
                self.assertEqual(run.call_args.args[0][0], "powershell.exe")
                self.assertTrue(run.call_args.kwargs["check"])
                self.assertEqual(run.call_args.kwargs["timeout"], 60)

    def test_acl_check_failure_is_not_ignored(self):
        with patch.object(smoke.subprocess, "run",
                          side_effect=subprocess.CalledProcessError(1, "powershell.exe")):
            with self.assertRaises(subprocess.CalledProcessError):
                smoke.verify_workspace_acl(ROOT / "var", {})

    def test_empty_json_response_is_accepted(self):
        response = MagicMock()
        response.__enter__.return_value = response
        response.__exit__.return_value = False
        response.status = 204
        response.headers.get_content_type.return_value = "application/json"
        response.read.return_value = b""
        with patch.object(smoke.urllib.request, "urlopen", return_value=response):
            self.assertIsNone(smoke.request("http://localhost", "/document",
                                            method="DELETE", expected=204))


if __name__ == "__main__":
    unittest.main()
