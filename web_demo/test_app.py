"""Offline unittest coverage: no test contacts a real Space."""

from __future__ import annotations

import asyncio
import io
import json
import re
import socket
import threading
import time
import unittest
import urllib.error
from contextlib import AsyncExitStack
from email.message import Message
from pathlib import Path
from unittest.mock import MagicMock, patch

import httpx

from web_demo.app import (
    CATALOG,
    FILE_PLACEHOLDER,
    MAX_UPLOAD,
    Control,
    GatewayError,
    GradioGateway,
    Model,
    NoRedirect,
    PinnedHTTPSConnection,
    Settings,
    create_app,
    read_json,
    read_sse,
    safe_upload_path,
    substitute_file,
    text_from_result,
    validate_origin,
)

PNG = b"\x89PNG\r\n\x1a\nsynthetic-test-fixture"
HEADERS = {
    "Content-Type": "image/png",
    "X-Model-Id": "lightonocr-3",
    "X-Cloud-Consent": "true",
}


def model(**overrides):
    value = {
        "id": "lightonocr-3",
        "origin": "https://operator-example.hf.space",
        "api_name": "/recognize",
        "inputs": [FILE_PLACEHOLDER, "transcribe", True],
    }
    value.update(overrides)
    return Model.parse(value)


def control():
    return Control(time.monotonic() + 5)


class Response(io.BytesIO):
    def __init__(self, body: bytes, mime: str = "application/json"):
        super().__init__(body)
        self.headers = Message()
        self.headers["Content-Type"] = mime


class FakeGateway:
    def __init__(self):
        self.calls = []

    def extract(self, selected, content, mime, state):
        self.calls.append((selected.id, content, mime))
        return ["Synthetic transcription", {"unaltered": True}]


class ConfigurationTests(unittest.TestCase):
    def test_disabled_configuration_ignores_secrets_and_providers(self):
        with patch.dict("os.environ", {
            "WEB_DEMO_ENABLED": "false",
            "WEB_DEMO_MODELS_JSON": "not-json",
            "WEB_DEMO_HF_TOKEN": "not-a-real-token",
        }, clear=True):
            config = Settings.from_env()
        self.assertFalse(config.enabled)
        self.assertEqual(config.models, {})
        self.assertEqual(config.token, "")

    def test_explicit_opt_in_and_exact_mapping(self):
        mapping = {
            "id": "lightonocr-3",
            "origin": "https://operator-example.hf.space",
            "api_name": "/recognize",
            "inputs": [FILE_PLACEHOLDER],
        }
        with patch.dict("os.environ", {
            "WEB_DEMO_ENABLED": "true",
            "WEB_DEMO_MODELS_JSON": json.dumps([mapping]),
        }, clear=True):
            config = Settings.from_env()
        self.assertTrue(config.enabled)
        self.assertEqual(config.models["lightonocr-3"].api_name, "recognize")
        self.assertNotIn("xiaomi-ocr-0", config.models)

    def test_invalid_origins(self):
        for origin in (
            "http://operator-example.hf.space",
            "https://localhost",
            "https://127.0.0.1",
            "https://hf.space",
            "https://x.hf.space.evil.example",
            "https://x.hf.space@127.0.0.1",
            "https://x.hf.space:443",
            "https://x.hf.space/path",
            "https://x.hf.space?next=x",
            "https://x.hf.space#fragment",
            "https://x.hf.space\n",
        ):
            with self.subTest(origin=origin), self.assertRaises(ValueError):
                validate_origin(origin)
        self.assertEqual(validate_origin("https://x.hf.space/"), "https://x.hf.space")

    def test_templates_are_plain_json_with_a_required_file_placeholder(self):
        selected = model(inputs=[{"image": FILE_PLACEHOLDER}, 42, None])
        self.assertEqual(
            substitute_file(selected.inputs, {"path": "/upload/file.png"}),
            [{"image": {"path": "/upload/file.png"}}, 42, None],
        )
        for overrides in (
            {"inputs": ["no file"]},
            {"inputs": FILE_PLACEHOLDER},
            {"api_name": "../../predict"},
            {"api_prefix": "/custom/../../"},
            {"id": "browser-supplied-url"},
            {"text_pointer": "not-a-pointer"},
        ):
            with self.subTest(overrides=overrides), self.assertRaises(ValueError):
                model(**overrides)

    def test_bounds_and_secret_repr(self):
        for kwargs in (
            {"concurrency": 0}, {"concurrency": 9},
            {"deadline_seconds": 0}, {"deadline_seconds": 181},
            {"deadline_seconds": float("nan")}, {"token": "bad\r\nheader"},
        ):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                Settings(**kwargs)
        self.assertNotIn("server-only-secret", repr(Settings(token="server-only-secret")))

    def test_duplicate_ids_rejected(self):
        mapping = {
            "id": "lightonocr-3", "origin": "https://x.hf.space",
            "api_name": "/predict", "inputs": [FILE_PLACEHOLDER],
        }
        with patch.dict("os.environ", {
            "WEB_DEMO_ENABLED": "true",
            "WEB_DEMO_MODELS_JSON": json.dumps([mapping, mapping]),
        }, clear=True), self.assertRaises(ValueError):
            Settings.from_env()

    def test_static_mount_cannot_expose_repository_or_backend(self):
        for directory in (
            Path.cwd(), Path.cwd() / "backend",
            Path.cwd() / "studio", Path.cwd() / "studio" / "dist",
        ):
            with self.subTest(directory=directory), self.assertRaises(ValueError):
                create_app(Settings(static_dir=directory))


class ProtocolTests(unittest.TestCase):
    def test_upload_call_and_sse_workflow_with_server_only_token(self):
        state = control()
        responses = [
            Response(b'["/var/gradio/abc/document.png"]'),
            Response(b'{"event_id":"abc-123"}'),
            Response(
                b"event: heartbeat\ndata: null\n\n"
                b"event: generating\ndata: [\"partial\"]\n\n"
                b"event: complete\ndata: [\"final server-only-secret\", {\"file\":"
                b" \"https://never-fetch.example/result\"}]\n\n",
                "text/event-stream",
            ),
        ]
        opener = MagicMock()
        opener.open.side_effect = responses
        settings = Settings(enabled=True, token="server-only-secret")
        with patch("web_demo.app.urllib.request.build_opener", return_value=opener):
            result = GradioGateway(settings).extract(model(), PNG, "image/png", state)
        self.assertEqual(result[0], "final [redacted]")
        self.assertEqual(result[1]["file"], "https://never-fetch.example/result")
        requests = [call.args[0] for call in opener.open.call_args_list]
        self.assertEqual([req.full_url for req in requests], [
            "https://operator-example.hf.space/gradio_api/upload",
            "https://operator-example.hf.space/gradio_api/call/recognize",
            "https://operator-example.hf.space/gradio_api/call/recognize/abc-123",
        ])
        self.assertIn(b'name="files"; filename="document.png"', requests[0].data)
        self.assertIn(PNG, requests[0].data)
        self.assertEqual(
            json.loads(requests[1].data)["data"],
            [{
                "path": "/var/gradio/abc/document.png",
                "orig_name": "document.png",
                "mime_type": "image/png",
                "meta": {"_type": "gradio.FileData"},
            }, "transcribe", True],
        )
        for req in requests:
            self.assertEqual(req.get_header("Authorization"), "Bearer " + "server-only-secret")
            self.assertNotIn(b"server-only-secret", req.data or b"")
        self.assertTrue(all(response.closed for response in responses))
        self.assertTrue(state.cancelled.is_set())

    def test_legacy_prefix_is_explicit_not_fallback(self):
        opener = MagicMock()
        opener.open.side_effect = [
            Response(b'["/gradio/document.pdf"]'),
            Response(b'{"event_id":"123"}'),
            Response(b'event: complete\ndata: ["text"]\n\n', "text/event-stream"),
        ]
        with patch("web_demo.app.urllib.request.build_opener", return_value=opener):
            result = GradioGateway(Settings()).extract(
                model(api_prefix=""), b"%PDF-synthetic", "application/pdf", control()
            )
        self.assertEqual(result, ["text"])
        self.assertEqual(opener.open.call_args_list[0].args[0].full_url,
                         "https://operator-example.hf.space/upload")

    def test_upload_paths_never_allow_urls_or_traversal(self):
        for value in (
            "https://127.0.0.1/file", "//127.0.0.1/file", "/a/../secret",
            "/a/./file", "/a//file", "/file?url=x", "/file#x", "/a\\b",
            "/path\x00file", "/file:other", "relative.png", None,
        ):
            with self.subTest(value=value), self.assertRaises(GatewayError):
                safe_upload_path(value)
        self.assertEqual(safe_upload_path("/var/gradio/a/image.png"), "/var/gradio/a/image.png")

    def test_upload_url_response_is_not_followed(self):
        opener = MagicMock()
        opener.open.return_value = Response(b'["http://127.0.0.1/private"]')
        with patch("web_demo.app.urllib.request.build_opener", return_value=opener):
            with self.assertRaises(GatewayError):
                GradioGateway(Settings()).extract(model(), PNG, "image/png", control())
        self.assertEqual(opener.open.call_count, 1)

    def test_invalid_event_id_cannot_change_request_path(self):
        opener = MagicMock()
        opener.open.side_effect = [
            Response(b'["/gradio/document.png"]'),
            Response(b'{"event_id":"../../private"}'),
        ]
        with patch("web_demo.app.urllib.request.build_opener", return_value=opener):
            with self.assertRaises(GatewayError):
                GradioGateway(Settings()).extract(model(), PNG, "image/png", control())
        self.assertEqual(opener.open.call_count, 2)

    def test_upstream_error_is_not_leaked_or_retried(self):
        opener = MagicMock()
        error = urllib.error.HTTPError(
            "https://x.hf.space", 403, "server-only-secret", {}, io.BytesIO(b"private")
        )
        opener.open.side_effect = error
        with patch("web_demo.app.urllib.request.build_opener", return_value=opener):
            with self.assertRaises(GatewayError) as caught:
                GradioGateway(Settings()).extract(model(), PNG, "image/png", control())
        self.assertNotIn("secret", str(caught.exception))
        self.assertEqual(opener.open.call_count, 1)
        self.assertTrue(error.closed)

    def test_redirects_are_not_followed(self):
        self.assertIsNone(NoRedirect().redirect_request(
            None, None, 302, "redirect", {}, "http://127.0.0.1"
        ))

    def test_private_dns_results_are_rejected_before_socket_connect(self):
        for address in ("127.0.0.1", "10.0.0.1", "169.254.169.254", "::1", "100.64.0.1"):
            with self.subTest(address=address):
                connection = PinnedHTTPSConnection("x.hf.space", control=control())
                with patch("web_demo.app.socket.getaddrinfo", return_value=[
                    (socket.AF_INET, socket.SOCK_STREAM, 6, "", (address, 443))
                ]), patch("web_demo.app.socket.socket") as socket_factory:
                    with self.assertRaises(GatewayError):
                        connection.connect()
                    socket_factory.assert_not_called()

    def test_mixed_public_private_dns_results_rejected(self):
        connection = PinnedHTTPSConnection("x.hf.space", control=control())
        with patch("web_demo.app.socket.getaddrinfo", return_value=[
            (socket.AF_INET, socket.SOCK_STREAM, 6, "", ("1.1.1.1", 443)),
            (socket.AF_INET, socket.SOCK_STREAM, 6, "", ("127.0.0.1", 443)),
        ]), self.assertRaises(GatewayError):
            connection.connect()

    def test_public_dns_is_pinned_while_tls_uses_original_hostname(self):
        state = control()
        context = MagicMock()
        raw_socket = MagicMock()
        tls_socket = context.wrap_socket.return_value
        connection = PinnedHTTPSConnection(
            "operator-example.hf.space", control=state, context=context
        )
        with patch("web_demo.app.socket.getaddrinfo", return_value=[
            (socket.AF_INET, socket.SOCK_STREAM, 6, "", ("1.1.1.1", 443))
        ]) as resolve, patch("web_demo.app.socket.socket", return_value=raw_socket):
            connection.connect()
        resolve.assert_called_once_with(
            "operator-example.hf.space", 443, type=socket.SOCK_STREAM
        )
        raw_socket.connect.assert_called_once_with(("1.1.1.1", 443))
        context.wrap_socket.assert_called_once_with(
            raw_socket, server_hostname="operator-example.hf.space",
            do_handshake_on_connect=False,
        )
        tls_socket.do_handshake.assert_called_once()
        state.cancel()
        tls_socket.shutdown.assert_called_once_with(socket.SHUT_RDWR)
        tls_socket.close.assert_called_once()

    def test_sse_multiline_json_and_output_pointer(self):
        data = read_sse(
            Response(b":comment\n\nevent: complete\ndata: [{\"text\":\ndata: \"hello\"}]\n\n"),
            control(),
        )
        self.assertEqual(text_from_result(data, "/0/text"), "hello")
        self.assertEqual(text_from_result([{"a/b": {"~": "text"}}], "/0/a~1b/~0"), "text")
        self.assertEqual(json.loads(text_from_result(data, "")), data)
        with self.assertRaises(GatewayError):
            text_from_result(data, "/missing")

    def test_sse_errors_and_invalid_outputs(self):
        for content in (
            b"event: error\ndata: secret-error\n\n",
            b"event: complete\ndata: invalid-json\n\n",
            b'event: complete\ndata: {"not":"list"}\n\n',
            b"event: generating\ndata: []\n\n",
            b"event: complete\ndata: [NaN]\n\n",
            b"event: complete\ndata: [1e400]\n\n",
            b"event: complete\ndata: \xff\n\n",
        ):
            with self.subTest(content=content), self.assertRaises(GatewayError):
                read_sse(Response(content), control())

    def test_response_byte_event_and_line_limits(self):
        with patch("web_demo.app.MAX_LINE", 10), self.assertRaises(GatewayError):
            read_sse(Response(b"event: " + b"x" * 12), control())
        with patch("web_demo.app.MAX_EVENTS", 1), self.assertRaises(GatewayError):
            read_sse(Response(b"\n\n"), control())
        with patch("web_demo.app.MAX_UPSTREAM", 10), self.assertRaises(GatewayError):
            read_sse(Response(b"event: heartbeat\n\n"), control())
        with self.assertRaises(GatewayError):
            read_json(Response(b"x" * (64 * 1024 + 1)), control())

    def test_cancellation_closes_registered_socket(self):
        state = control()
        sock = MagicMock()
        state.register(sock)
        state.cancel()
        sock.shutdown.assert_called_once_with(socket.SHUT_RDWR)
        sock.close.assert_called_once()
        with self.assertRaises(GatewayError):
            state.check()
        with self.assertRaises(GatewayError):
            state.register(MagicMock())

    def test_cancellation_interrupts_real_blocking_stream_socket(self):
        state = control()
        local, peer = socket.socketpair()
        state.register(local)
        ready = threading.Event()
        completed = threading.Event()
        errors = []

        def read():
            try:
                with local.makefile("rb") as stream:
                    ready.set()
                    read_sse(stream, state)
            except GatewayError as exc:
                errors.append(exc)
            finally:
                completed.set()

        reader = threading.Thread(target=read)
        try:
            reader.start()
            self.assertTrue(ready.wait(1))
            state.cancel()
            self.assertTrue(completed.wait(1))
            self.assertEqual(len(errors), 1)
        finally:
            state.cancel()
            peer.close()
            reader.join(1)


class ApiTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.stack = AsyncExitStack()
        self.fake = FakeGateway()
        self.settings = Settings(enabled=True, models={"lightonocr-3": model()})
        self.app = create_app(self.settings, self.fake)
        await self.stack.enter_async_context(self.app.router.lifespan_context(self.app))
        self.client = await self.stack.enter_async_context(httpx.AsyncClient(
            transport=httpx.ASGITransport(app=self.app), base_url="http://demo.test"
        ))

    async def asyncTearDown(self):
        await self.stack.aclose()

    async def test_catalog_and_successful_raw_upload(self):
        response = await self.client.get("/demo-api/models")
        entries = response.json()["models"]
        self.assertEqual([item["id"] for item in entries], list(CATALOG))
        self.assertTrue(entries[0]["available"])
        self.assertFalse(entries[1]["available"])
        self.assertIn("reason", entries[1])
        response = await self.client.post("/demo-api/extract", content=PNG, headers=HEADERS)
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json()["model"], "lightonocr-3")
        self.assertEqual(response.json()["text"], "Synthetic transcription")
        self.assertEqual(response.json()["data"], ["Synthetic transcription", {"unaltered": True}])
        self.assertNotIn("regions", response.json())
        self.assertIsInstance(response.json()["elapsed_ms"], int)
        self.assertEqual(self.fake.calls, [("lightonocr-3", PNG, "image/png")])

    async def test_gateway_disabled_by_default(self):
        disabled = create_app(Settings(), self.fake)
        async with disabled.router.lifespan_context(disabled):
            async with httpx.AsyncClient(
                transport=httpx.ASGITransport(app=disabled), base_url="http://demo.test"
            ) as client:
                catalog = (await client.get("/demo-api/models")).json()
                self.assertTrue(all(not entry["available"] for entry in catalog["models"]))
                response = await client.post("/demo-api/extract", content=PNG, headers=HEADERS)
                self.assertEqual(response.status_code, 503)
        self.assertEqual(self.fake.calls, [])

    async def test_consent_and_model_are_required(self):
        for headers in (
            {**HEADERS, "X-Cloud-Consent": "false"},
            {**HEADERS, "X-Cloud-Consent": ""},
            {**HEADERS, "X-Model-Id": "xiaomi-ocr-0"},
            {**HEADERS, "X-Model-Id": "https://x.hf.space"},
        ):
            with self.subTest(headers=headers):
                response = await self.client.post("/demo-api/extract", content=PNG, headers=headers)
                self.assertEqual(response.status_code, 400)
        self.assertEqual(self.fake.calls, [])

    async def test_mime_and_signature_checks(self):
        for mime, body in (
            ("text/html", b"<html>"), ("image/png", b"not-a-png"),
            ("application/pdf", PNG), ("image/jpeg", b"plain text"),
        ):
            with self.subTest(mime=mime):
                response = await self.client.post(
                    "/demo-api/extract", content=body, headers={**HEADERS, "Content-Type": mime}
                )
                self.assertEqual(response.status_code, 415)
        for mime, body in (
            ("image/jpeg", b"\xff\xd8\xffsynthetic"),
            ("application/pdf", b"%PDF-synthetic"),
        ):
            response = await self.client.post(
                "/demo-api/extract", content=body, headers={**HEADERS, "Content-Type": mime}
            )
            self.assertEqual(response.status_code, 200)

    async def test_declared_size_rejected_before_upload(self):
        response = await self.client.post(
            "/demo-api/extract", content=PNG,
            headers={**HEADERS, "Content-Length": str(MAX_UPLOAD + 1)},
        )
        self.assertEqual(response.status_code, 413)
        response = await self.client.post(
            "/demo-api/extract", content=PNG,
            headers={**HEADERS, "Content-Length": "-1"},
        )
        self.assertEqual(response.status_code, 400)
        self.assertEqual(self.fake.calls, [])

    async def test_streamed_size_is_enforced_without_content_length(self):
        yielded = []

        async def chunks():
            yielded.append(1)
            yield PNG
            yielded.append(2)
            yield b"x" * MAX_UPLOAD
            yielded.append(3)
            yield b"must-not-be-consumed"

        response = await self.client.post("/demo-api/extract", content=chunks(), headers=HEADERS)
        self.assertEqual(response.status_code, 413)
        self.assertEqual(yielded, [1, 2])
        self.assertEqual(self.fake.calls, [])

    async def test_exact_upload_limit_is_accepted(self):
        content = PNG + b"x" * (MAX_UPLOAD - len(PNG))
        response = await self.client.post(
            "/demo-api/extract", content=content, headers=HEADERS
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(len(self.fake.calls[0][1]), MAX_UPLOAD)

    async def test_client_disconnect_cancels_worker(self):
        started = threading.Event()
        cleaned = threading.Event()
        disconnected = asyncio.Event()

        class BlockingGateway:
            def extract(self, selected, content, mime, state):
                started.set()
                state.cancelled.wait(2)
                cleaned.set()
                state.check()

        app = create_app(self.settings, BlockingGateway())
        messages = []
        body_sent = False

        async def receive():
            nonlocal body_sent
            if not body_sent:
                body_sent = True
                return {"type": "http.request", "body": PNG, "more_body": False}
            await disconnected.wait()
            return {"type": "http.disconnect"}

        async def send(message):
            messages.append(message)

        scope = {
            "type": "http", "asgi": {"version": "3.0"}, "http_version": "1.1",
            "method": "POST", "scheme": "http", "path": "/demo-api/extract",
            "raw_path": b"/demo-api/extract", "query_string": b"",
            "root_path": "", "server": ("demo.test", 80), "client": ("127.0.0.1", 1),
            "headers": [(key.lower().encode(), value.encode()) for key, value in HEADERS.items()],
        }
        async with app.router.lifespan_context(app):
            task = asyncio.create_task(app(scope, receive, send))
            for _ in range(100):
                if started.is_set():
                    break
                await asyncio.sleep(0.01)
            self.assertTrue(started.is_set())
            disconnected.set()
            await asyncio.wait_for(task, 1)
            for _ in range(100):
                if cleaned.is_set():
                    break
                await asyncio.sleep(0.01)
            self.assertTrue(cleaned.is_set())
            self.assertEqual(
                next(message["status"] for message in messages
                     if message["type"] == "http.response.start"), 499
            )

    async def test_cross_origin_and_offline_routes_are_unavailable(self):
        response = await self.client.post(
            "/demo-api/extract", content=PNG,
            headers={**HEADERS, "Origin": "https://untrusted.example"},
        )
        self.assertEqual(response.status_code, 403)
        self.assertNotIn("access-control-allow-origin", response.headers)
        response = await self.client.get("/demo-api/models", headers={"Origin": "null"})
        self.assertEqual(response.status_code, 403)
        response = await self.client.get("/demo-api/models", headers={"Origin": "http://demo.test"})
        self.assertEqual(response.status_code, 200)
        for route in ("/api/jobs", "/api/documents", "/docs", "/openapi.json", "/backend"):
            self.assertEqual((await self.client.get(route)).status_code, 404)

    async def test_isolated_studio_build_html_and_root_assets(self):
        directory = Path(__file__).resolve().parent.parent / "studio" / "dist-demo"
        if not (directory / "demo" / "index.html").is_file():
            self.skipTest("Separate demo build is not present")
        app = create_app(Settings(static_dir=directory))
        async with app.router.lifespan_context(app):
            async with httpx.AsyncClient(
                transport=httpx.ASGITransport(app=app), base_url="http://demo.test"
            ) as client:
                root = await client.get("/")
                self.assertEqual(root.status_code, 307)
                self.assertEqual(root.headers["location"], "/demo/index.html")
                html = await client.get("/demo/index.html")
                self.assertEqual(html.status_code, 200)
                self.assertIn("text/html", html.headers["content-type"])
                self.assertEqual((await client.get("/demo/")).status_code, 200)
                assets = re.findall(r'(?:src|href)="(/assets/[^"]+)"', html.text)
                self.assertTrue(assets)
                for asset in assets:
                    self.assertEqual((await client.get(asset)).status_code, 200)
                catalog = await client.get("/demo-api/models")
                self.assertEqual(catalog.status_code, 200)
                self.assertIn("models", catalog.json())
                for route in ("/api/jobs", "/backend/app/main.py", "/studio/dist/index.html"):
                    self.assertEqual((await client.get(route)).status_code, 404)

    async def test_concurrency_deadline_and_cleanup(self):
        started = threading.Event()
        cleaned = threading.Event()

        class BlockingGateway:
            def extract(self, selected, content, mime, state):
                started.set()
                try:
                    while not state.cancelled.wait(0.01):
                        state.check()
                    state.check()
                finally:
                    cleaned.set()

        app = create_app(
            Settings(enabled=True, models={"lightonocr-3": model()},
                     concurrency=1, deadline_seconds=1),
            BlockingGateway(),
        )
        async with app.router.lifespan_context(app):
            async with httpx.AsyncClient(
                transport=httpx.ASGITransport(app=app), base_url="http://demo.test"
            ) as client:
                first = asyncio.create_task(client.post(
                    "/demo-api/extract", content=PNG, headers=HEADERS
                ))
                for _ in range(100):
                    if started.is_set():
                        break
                    await asyncio.sleep(0.01)
                self.assertTrue(started.is_set())
                busy = await client.post("/demo-api/extract", content=PNG, headers=HEADERS)
                self.assertEqual(busy.status_code, 429)
                response = await first
                self.assertEqual(response.status_code, 504)
                for _ in range(100):
                    if cleaned.is_set():
                        break
                    await asyncio.sleep(0.01)
                self.assertTrue(cleaned.is_set())


if __name__ == "__main__":
    unittest.main()
