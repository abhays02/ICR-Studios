import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[1]


class DocsWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = yaml.load(
            (ROOT / ".github/workflows/docs.yml").read_text(),
            Loader=yaml.BaseLoader,
        )
        cls.deploy = cls.workflow["jobs"]["deploy"]
        cls.check = next(step for step in cls.deploy["steps"] if step.get("id") == "pages")

    def run_check(self, status, curl_exit=0):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            result = subprocess.run(
                ["bash", "-e", "-c",
                 'curl() { [[ "$*" == *"$GH_TOKEN"* ]] || return 99; '
                 'printf "%s" "$HTTP_STATUS"; return "$CURL_EXIT"; }\n'
                 + self.check["run"]],
                env={**os.environ, "HTTP_STATUS": str(status), "CURL_EXIT": str(curl_exit),
                     "GH_TOKEN": "test-placeholder", "PAGES_API_URL": "https://api.invalid/pages",
                     "GITHUB_OUTPUT": str(output)},
                capture_output=True, text=True,
            )
            return result, output.read_text() if output.exists() else ""

    def test_enabled_pages_can_deploy(self):
        result, output = self.run_check(200)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, "enabled=true\n")
        deployment = next(step for step in self.deploy["steps"] if step.get("id") == "deployment")
        self.assertEqual(deployment["if"], "steps.pages.outputs.enabled == 'true'")
        self.assertLess(self.deploy["steps"].index(self.check),
                        self.deploy["steps"].index(deployment))

    def test_missing_pages_skips_deployment_with_setup_warning(self):
        result, output = self.run_check(404)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, "enabled=false\n")
        self.assertIn("::warning::", result.stdout)
        self.assertIn("Settings > Pages", result.stdout)

    def test_other_api_failures_are_not_hidden(self):
        for status in (401, 403, 429, 500):
            with self.subTest(status=status):
                result, output = self.run_check(status)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f"::error::Failed to check GitHub Pages availability (HTTP {status})",
                              result.stdout)
                self.assertEqual(output, "")

    def test_transport_errors_fail_closed(self):
        result, output = self.run_check("000", curl_exit=7)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output, "")

    def test_publishing_remains_restricted_to_default_branch_without_prs(self):
        upload = next(step for step in self.workflow["jobs"]["build"]["steps"]
                      if step.get("uses", "").startswith("actions/upload-pages-artifact@"))
        self.assertEqual(upload["if"], self.deploy["if"])
        self.assertEqual(
            self.deploy["if"],
            "(github.event_name == 'push' || (github.event_name == 'workflow_dispatch' "
            "&& inputs.publish)) && github.ref_name == github.event.repository.default_branch",
        )
        self.assertEqual(self.workflow["permissions"], {"contents": "read"})
        self.assertEqual(self.deploy["permissions"], {"pages": "write", "id-token": "write"})
