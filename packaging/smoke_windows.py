"""Real executable smoke, run on Windows CI after extracting the release ZIP."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request


def request(base, path, token=None, method="GET", body=None, media="application/json", expected=200):
    headers = {"Content-Type": media}
    if token:
        headers["Authorization"] = "Bearer " + token
    if isinstance(body, dict):
        body = json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=body, headers=headers, method=method)
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as exc:
        response = exc
    with response:
        if response.status != expected:
            raise AssertionError(f"{method} {path}: expected {expected}, got {response.status}")
        raw = response.read()
        if not raw:
            return None
        if response.headers.get_content_type() == "application/json":
            return json.loads(raw)
        return raw


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("executable", type=Path)
    parser.add_argument("--root", type=Path, default=Path("var/windows-smoke"))
    args = parser.parse_args()
    if sys.platform != "win32":
        raise SystemExit("Authentic executable smoke requires Windows")
    executable = args.executable.resolve()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    unrelated = root / "unrelated-working-directory"
    unrelated.mkdir()
    workspace = root / "private-workspace"
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("ICR_")}
    command = [str(executable), "--workspace", str(workspace)]
    subprocess.run(command + ["init-secrets"], cwd=unrelated, env=env, check=True, timeout=60)
    secret_file = workspace / "var/windows-secrets.json"
    before = secret_file.read_bytes()
    duplicate = subprocess.run(command + ["init-secrets"], cwd=unrelated, env=env,
                               capture_output=True, timeout=60)
    assert duplicate.returncode != 0 and secret_file.read_bytes() == before
    token = next(iter(json.loads(json.loads(before)["ICR_TOKENS"])))
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    document_id = None
    for restart in range(2):
        with (root / f"server-{restart}.log").open("wb") as log:
            process = subprocess.Popen(command + ["--port", str(port), "serve"], cwd=unrelated,
                                       env=env, stdout=log, stderr=subprocess.STDOUT)
            try:
                for _ in range(90):
                    if process.poll() is not None:
                        raise AssertionError("Executable exited before health; inspect local smoke log")
                    try:
                        assert request(base, "/healthz") == {"status": "alive"}
                        break
                    except (OSError, AssertionError):
                        time.sleep(1)
                else:
                    raise AssertionError("Executable startup timed out")
                ready = request(base, "/readyz")
                assert ready["api"] == "ready" and ready["human_review_available"]
                assert ready["inference"] != "ready"
                html = request(base, "/").decode()
                assert "ICR Studio" in html
                assets = re.findall(r'(?:src|href)="(/assets/[^"]+)"', html)
                assert assets and all(request(base, asset) for asset in assets)
                request(base, "/api/v1/documents", expected=401)
                request(base, "/api/v1/does-not-exist", expected=404)
                request(base, "/var/windows-secrets.json", expected=404)
                request(base, "/_internal/config/windows.yaml", expected=404)
                templates = request(base, "/api/v1/templates", token)["items"]
                assert len(templates) >= 5
                samples = request(base, "/api/v1/samples", token)
                assert samples["synthetic"] and not samples["is_real_handwriting"]
                if restart:
                    result = request(base, f"/api/v1/documents/{document_id}/results", token)
                    assert result["document_id"] == document_id and result["fields"]
                    assert result["job_state"] == "failed"
                    request(base, f"/api/v1/documents/{document_id}", token, "DELETE", expected=204)
                    assert request(base, "/api/v1/documents", token)["items"] == []
                    continue
                sample = next(item for item in samples["items"] if not item["alignment_required"])
                identifier = sample["sample_id"]
                image = request(base, f"/api/v1/samples/{identifier}/image", token)
                imported = request(base, f"/api/v1/samples/{identifier}/import",
                                   token, "POST", expected=201)
                document_id = imported["id"]
                assert imported["synthetic"]
                template = next(item for item in templates if item["template_id"] == imported["template_id"])
                uploaded = request(base, "/api/v1/documents?template_id=" + template["template_id"]
                                   + "&template_version=" + template["template_version"],
                                   token, "POST", image, "image/png", 201)
                assert not uploaded["synthetic"]
                request(base, f"/api/v1/documents/{uploaded['id']}", token, "DELETE", expected=204)
                job = request(base, f"/api/v1/documents/{document_id}/jobs", token, "POST",
                              {"idempotency_key": "windows-smoke-missing-model",
                               "alignment_confirmed": True}, expected=202)
                for _ in range(90):
                    status = request(base, f"/api/v1/jobs/{job['id']}", token)
                    if status["state"] in {"failed", "partial", "needs_review", "completed"}:
                        break
                    time.sleep(1)
                assert status["state"] == "failed"
                issues = {issue for field in status["fields"].values() for issue in field["issues"]}
                assert "model_unavailable" in issues
            finally:
                process.terminate()
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)
        for path in (workspace / "var").glob("icr.sqlite3*"):
            assert b"\x89PNG\r\n\x1a\n" not in path.read_bytes()
        for path in root.glob("server-*.log"):
            assert token.encode() not in path.read_bytes()
    assert not (unrelated / "var").exists()
    assert secret_file.read_bytes() == before
    acl_env = dict(env, ICR_SMOKE_WORKSPACE=str(workspace))
    subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", """
        $ErrorActionPreference = 'Stop'
        $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $paths = @((Get-Item -LiteralPath $env:ICR_SMOKE_WORKSPACE)) +
            @(Get-ChildItem -LiteralPath $env:ICR_SMOKE_WORKSPACE -Recurse -Force)
        foreach ($path in $paths) {
            $acl = Get-Acl -LiteralPath $path.FullName
            foreach ($rule in $acl.Access) {
                $identity = $rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
                if ($identity -ne $sid -and $identity -ne 'S-1-5-18') { throw 'Unexpected workspace ACL' }
            }
            if (-not $acl.AreAccessRulesProtected -and $path.FullName -eq $env:ICR_SMOKE_WORKSPACE) {
                throw 'Workspace root ACL is not protected'
            }
        }
    """], env=acl_env, check=True, timeout=60)
    manifest = json.loads((executable.parent / "bundle-manifest.json").read_text(encoding="utf-8"))
    for name, digest in manifest["files"].items():
        assert hashlib.sha256((executable.parent / name).read_bytes()).hexdigest() == digest
    print("Verified extracted Windows executable: health, offline Studio, auth, templates, "
          "synthetic import, real PNG upload, missing-model subprocess, encrypted persistence/restart, "
          "deletion and private ACLs.")


if __name__ == "__main__":
    main()
