"""A bounded, opt-in Gradio HTTP gateway, independent of the offline backend."""

from __future__ import annotations

import asyncio
import concurrent.futures
import contextlib
import copy
import ipaddress
import json
import math
import os
import re
import socket
import ssl
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from contextlib import asynccontextmanager
from dataclasses import dataclass, field
from http.client import HTTPException as HTTPProtocolError, HTTPSConnection
from pathlib import Path, PurePosixPath
from typing import Any

from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse, RedirectResponse
from fastapi.staticfiles import StaticFiles
from starlette.requests import ClientDisconnect

MAX_UPLOAD = 10 * 1024 * 1024
MAX_UPSTREAM = 2 * 1024 * 1024
MAX_LINE = 128 * 1024
MAX_EVENTS = 2048
FILE_PLACEHOLDER = "$uploaded_file"
CATALOG = {
    "lightonocr-3": "LightOnOCR 3",
    "xiaomi-ocr-0": "Xiaomi OCR 0",
}
UNAVAILABLE_REASONS = {
    "lightonocr-3": "Official LightOnOCR 3 identity is known; Gradio inference Space API is unverified and not configured",
    "xiaomi-ocr-0": "Official Xiaomi-OCR-0 identity is known; Gradio inference Space API is unverified and not configured",
}
MIME_TYPES = {
    "image/png": (".png", b"\x89PNG\r\n\x1a\n"),
    "image/jpeg": (".jpg", b"\xff\xd8\xff"),
    "application/pdf": (".pdf", b"%PDF-"),
}


class GatewayError(Exception):
    def __init__(self, status: int, message: str):
        self.status = status
        self.message = message


def load_json(value: str | bytes) -> Any:
    def invalid_constant(_: str) -> None:
        raise ValueError("Non-finite JSON value")

    def finite_float(raw: str) -> float:
        number = float(raw)
        if not math.isfinite(number):
            raise ValueError("Non-finite JSON value")
        return number

    return json.loads(value, parse_constant=invalid_constant, parse_float=finite_float)


def validate_origin(origin: str) -> str:
    if not isinstance(origin, str) or any(ord(char) <= 32 for char in origin):
        raise ValueError("Invalid origin")
    parsed = urllib.parse.urlsplit(origin)
    if (
        parsed.scheme != "https"
        or not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.hf\.space", parsed.netloc)
        or parsed.path not in ("", "/")
        or parsed.query
        or parsed.fragment
    ):
        raise ValueError("Origin must be a bare HTTPS *.hf.space origin")
    return f"https://{parsed.netloc}"


def validate_template(value: Any, depth: int = 0) -> int:
    if depth > 20:
        raise ValueError("Input template is too deeply nested")
    if isinstance(value, str):
        return int(value == FILE_PLACEHOLDER)
    if isinstance(value, list):
        return sum(validate_template(item, depth + 1) for item in value)
    if isinstance(value, dict):
        return sum(validate_template(item, depth + 1) for item in value.values())
    if value is None or isinstance(value, (bool, int)):
        return 0
    if isinstance(value, float) and math.isfinite(value):
        return 0
    raise ValueError("Inputs must be JSON values")


def substitute_file(value: Any, file_data: dict[str, Any]) -> Any:
    if isinstance(value, str) and value == FILE_PLACEHOLDER:
        return copy.deepcopy(file_data)
    if isinstance(value, list):
        return [substitute_file(item, file_data) for item in value]
    if isinstance(value, dict):
        return {key: substitute_file(item, file_data) for key, item in value.items()}
    return value


@dataclass(frozen=True)
class Model:
    id: str
    label: str
    origin: str
    api_name: str
    inputs: list[Any]
    api_prefix: str = "/gradio_api"
    text_pointer: str = "/0"

    @classmethod
    def parse(cls, value: dict[str, Any]) -> "Model":
        if not isinstance(value, dict) or value.get("id") not in CATALOG:
            raise ValueError("Unknown catalog id")
        label = value.get("label", CATALOG[value["id"]])
        if not isinstance(label, str) or not 1 <= len(label) <= 120:
            raise ValueError("Invalid label")
        origin = validate_origin(value["origin"])
        api_name = value["api_name"]
        if not isinstance(api_name, str) or not re.fullmatch(r"/?[A-Za-z0-9_-]{1,80}", api_name):
            raise ValueError("Configure an exact, single-segment Gradio api_name")
        prefix = value.get("api_prefix", "/gradio_api")
        if prefix not in ("", "/gradio_api"):
            raise ValueError("api_prefix must be empty or /gradio_api")
        inputs = value["inputs"]
        if not isinstance(inputs, list) or not validate_template(inputs):
            raise ValueError("Inputs must contain an uploaded-file placeholder")
        pointer = value.get("text_pointer", "/0")
        if not isinstance(pointer, str) or (pointer and not pointer.startswith("/")):
            raise ValueError("text_pointer must be a JSON Pointer")
        return cls(value["id"], label, origin, api_name.lstrip("/"), inputs, prefix, pointer)


@dataclass(frozen=True)
class Settings:
    enabled: bool = False
    models: dict[str, Model] = field(default_factory=dict)
    token: str = field(default="", repr=False)
    concurrency: int = 2
    deadline_seconds: float = 90
    static_dir: Path | None = None

    def __post_init__(self) -> None:
        if not 1 <= self.concurrency <= 8:
            raise ValueError("Concurrency must be between 1 and 8")
        if not math.isfinite(self.deadline_seconds) or not 1 <= self.deadline_seconds <= 180:
            raise ValueError("Deadline must be between 1 and 180 seconds")
        if "\r" in self.token or "\n" in self.token:
            raise ValueError("Invalid server credential")

    @classmethod
    def from_env(cls) -> "Settings":
        enabled = os.getenv("WEB_DEMO_ENABLED", "").lower() == "true"
        models: dict[str, Model] = {}
        # Disabled deployments ignore all provider configuration and credentials.
        if enabled:
            config = os.getenv("WEB_DEMO_MODELS_JSON", "[]")
            if len(config.encode()) > 64 * 1024:
                raise ValueError("Provider configuration is too large")
            entries = load_json(config)
            if not isinstance(entries, list):
                raise ValueError("Provider configuration must be a list")
            for entry in entries:
                model = Model.parse(entry)
                if model.id in models:
                    raise ValueError("Duplicate model id")
                models[model.id] = model
        static = os.getenv("WEB_DEMO_STATIC_DIR")
        return cls(
            enabled=enabled,
            models=models,
            token=os.getenv("WEB_DEMO_HF_TOKEN", "") if enabled else "",
            concurrency=int(os.getenv("WEB_DEMO_CONCURRENCY", "2")),
            deadline_seconds=float(os.getenv("WEB_DEMO_DEADLINE_SECONDS", "90")),
            static_dir=Path(static).resolve() if static else None,
        )


class Control:
    """Shared cancellation/deadline state; shutdown interrupts a blocking SSE read."""

    def __init__(self, deadline: float):
        self.deadline = deadline
        self.cancelled = threading.Event()
        self.lock = threading.Lock()
        self.sockets: set[socket.socket] = set()
        self.bytes_read = 0

    def check(self) -> float:
        if self.cancelled.is_set():
            raise GatewayError(499, "Request cancelled")
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise GatewayError(504, "Cloud request deadline exceeded")
        return remaining

    def register(self, sock: socket.socket) -> None:
        with self.lock:
            if self.cancelled.is_set():
                sock.close()
                raise GatewayError(499, "Request cancelled")
            self.sockets.add(sock)

    def unregister(self, sock: socket.socket) -> None:
        with self.lock:
            self.sockets.discard(sock)

    def cancel(self) -> None:
        self.cancelled.set()
        with self.lock:
            for sock in self.sockets:
                with contextlib.suppress(OSError):
                    sock.shutdown(socket.SHUT_RDWR)
                sock.close()
            self.sockets.clear()

    def count(self, amount: int) -> None:
        self.check()
        self.bytes_read += amount
        if self.bytes_read > MAX_UPSTREAM:
            raise GatewayError(502, "Cloud response exceeded size limit")


class PinnedHTTPSConnection(HTTPSConnection):
    def __init__(self, *args: Any, control: Control, **kwargs: Any):
        super().__init__(*args, **kwargs)
        self.control = control

    def connect(self) -> None:
        # Resolve once, reject every non-public answer, then connect to that IP
        # while preserving the original hostname for TLS verification and SNI.
        answers = socket.getaddrinfo(self.host, 443, type=socket.SOCK_STREAM)
        self.control.check()
        addresses = [ipaddress.ip_address(item[4][0]) for item in answers]
        if not addresses or any(
            not address.is_global or address.is_multicast or address.is_reserved
            for address in addresses
        ):
            raise GatewayError(502, "Cloud origin did not resolve to public addresses")
        last_error: OSError | None = None
        for family, socktype, proto, _, address in answers:
            sock = socket.socket(family, socktype, proto)
            self.control.register(sock)
            try:
                sock.settimeout(min(30, self.control.check()))
                sock.connect(address)
                wrapped = self._context.wrap_socket(
                    sock, server_hostname=self.host, do_handshake_on_connect=False
                )
                self.control.unregister(sock)
                self.control.register(wrapped)
                wrapped.do_handshake()
                self.sock = wrapped
                return
            except OSError as exc:
                last_error = exc
                sock.close()
                self.control.unregister(sock)
        raise last_error or OSError("Cloud connection failed")


class PinnedHTTPSHandler(urllib.request.HTTPSHandler):
    def __init__(self, control: Control):
        super().__init__(context=ssl.create_default_context())
        self.control = control

    def https_open(self, request: urllib.request.Request) -> Any:
        def connection(host: str, **kwargs: Any) -> PinnedHTTPSConnection:
            return PinnedHTTPSConnection(host, control=self.control, **kwargs)

        return self.do_open(connection, request, context=self._context)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args: Any, **kwargs: Any) -> None:
        return None


def safe_upload_path(value: Any) -> str:
    if (
        not isinstance(value, str)
        or not value.startswith("/")
        or value.startswith("//")
        or len(value) > 4096
        or any(ord(char) < 32 for char in value)
        or any(char in value for char in "\\:?#")
        or any(part in (".", "..") for part in value.split("/"))
        or str(PurePosixPath(value)) != value
    ):
        raise GatewayError(502, "Cloud upload returned an invalid file path")
    return value


def read_json(response: Any, control: Control) -> Any:
    raw = response.read(min(64 * 1024, MAX_UPSTREAM - control.bytes_read) + 1)
    control.count(len(raw))
    if len(raw) > 64 * 1024:
        raise GatewayError(502, "Cloud metadata exceeded size limit")
    try:
        return load_json(raw)
    except (ValueError, UnicodeError, RecursionError):
        raise GatewayError(502, "Cloud returned invalid JSON") from None


def read_sse(response: Any, control: Control) -> Any:
    event = ""
    data: list[str] = []
    events = 0
    while True:
        control.check()
        raw = response.readline(MAX_LINE + 1)
        control.count(len(raw))
        if len(raw) > MAX_LINE:
            raise GatewayError(502, "Cloud event exceeded line limit")
        if not raw:
            raise GatewayError(502, "Cloud stream ended without a result")
        try:
            line = raw.decode("utf-8").rstrip("\r\n")
        except UnicodeError:
            raise GatewayError(502, "Cloud returned invalid event encoding") from None
        if not line:
            events += 1
            if events > MAX_EVENTS:
                raise GatewayError(502, "Cloud stream exceeded event limit")
            if event == "error":
                raise GatewayError(502, "The selected cloud model failed")
            if event == "complete":
                try:
                    result = load_json("\n".join(data))
                except (ValueError, RecursionError):
                    raise GatewayError(502, "Cloud returned an invalid result") from None
                if not isinstance(result, list):
                    raise GatewayError(502, "Cloud result must be an output list")
                return result
            event, data = "", []
        elif line.startswith("event:"):
            event = line[6:].strip()
        elif line.startswith("data:"):
            data.append(line[5:].lstrip(" "))


def redact(value: Any, token: str) -> Any:
    if not token:
        return value
    if isinstance(value, str):
        return value.replace(token, "[redacted]")
    if isinstance(value, list):
        return [redact(item, token) for item in value]
    if isinstance(value, dict):
        return {str(redact(key, token)): redact(item, token) for key, item in value.items()}
    return value


def text_from_result(data: Any, pointer: str) -> str:
    try:
        selected = data
        for part in pointer.split("/")[1:] if pointer else []:
            key = part.replace("~1", "/").replace("~0", "~")
            if isinstance(selected, list):
                if not re.fullmatch(r"0|[1-9][0-9]*", key):
                    raise ValueError("Invalid list index")
                selected = selected[int(key)]
            else:
                selected = selected[key]
    except (KeyError, IndexError, TypeError, ValueError):
        raise GatewayError(502, "Configured text output was absent from the cloud result") from None
    return selected if isinstance(selected, str) else json.dumps(selected, ensure_ascii=False)


class GradioGateway:
    def __init__(self, settings: Settings):
        self.settings = settings

    def extract(self, model: Model, content: bytes, mime: str, control: Control) -> Any:
        opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), PinnedHTTPSHandler(control), NoRedirect()
        )
        headers = {"Accept": "application/json", "Accept-Encoding": "identity"}
        if self.settings.token:
            headers["Authorization"] = "Bearer " + self.settings.token

        def request(path: str, payload: bytes | None, content_type: str) -> Any:
            control.check()
            req = urllib.request.Request(
                model.origin + model.api_prefix + path,
                data=payload,
                headers={**headers, "Content-Type": content_type},
                method="POST" if payload is not None else "GET",
            )
            return opener.open(req, timeout=min(30, control.check()))

        filename = "document" + MIME_TYPES[mime][0]
        boundary = "demo-" + os.urandom(24).hex()
        body = (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="files"; filename="{filename}"\r\n'
            f"Content-Type: {mime}\r\n\r\n"
        ).encode() + content + f"\r\n--{boundary}--\r\n".encode()
        try:
            with request("/upload", body, f"multipart/form-data; boundary={boundary}") as response:
                uploaded = read_json(response, control)
            if not isinstance(uploaded, list) or len(uploaded) != 1:
                raise GatewayError(502, "Cloud upload did not return one file")
            path = safe_upload_path(uploaded[0])
            file_data = {
                "path": path,
                "orig_name": filename,
                "mime_type": mime,
                "meta": {"_type": "gradio.FileData"},
            }
            payload = json.dumps({"data": substitute_file(model.inputs, file_data)}).encode()
            with request(f"/call/{model.api_name}", payload, "application/json") as response:
                submitted = read_json(response, control)
            event_id = submitted.get("event_id") if isinstance(submitted, dict) else None
            if not isinstance(event_id, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", event_id):
                raise GatewayError(502, "Cloud returned an invalid event id")
            headers["Accept"] = "text/event-stream"
            with request(f"/call/{model.api_name}/{event_id}", None, "application/json") as response:
                if response.headers.get_content_type() != "text/event-stream":
                    raise GatewayError(502, "Cloud did not return an event stream")
                return redact(read_sse(response, control), self.settings.token)
        except urllib.error.HTTPError as exc:
            exc.close()
            raise GatewayError(502, "Selected cloud Space rejected the request") from None
        except (TimeoutError, socket.timeout):
            raise GatewayError(504, "Cloud request timed out") from None
        except (urllib.error.URLError, OSError, ValueError, RecursionError, HTTPProtocolError):
            raise GatewayError(502, "Selected cloud Space could not be reached") from None
        finally:
            control.cancel()


class SameOriginMiddleware:
    def __init__(self, app: Any):
        self.app = app

    async def __call__(self, scope: Any, receive: Any, send: Any) -> None:
        if scope["type"] != "http":
            await self.app(scope, receive, send)
            return
        request = Request(scope)
        origin = request.headers.get("origin")
        if origin and origin != f"{request.url.scheme}://{request.url.netloc}":
            response = JSONResponse(
                {"detail": "Cross-origin requests are not allowed"}, status_code=403
            )
            await response(scope, receive, send)
            return

        async def secure_send(message: Any) -> None:
            if message["type"] == "http.response.start":
                message["headers"] = [
                    *message.get("headers", []),
                    (b"cache-control", b"no-store"),
                    (b"x-content-type-options", b"nosniff"),
                    (b"referrer-policy", b"no-referrer"),
                ]
            await send(message)

        await self.app(scope, receive, secure_send)


def create_app(settings: Settings | None = None, gateway: GradioGateway | None = None) -> FastAPI:
    settings = settings or Settings.from_env()
    gateway = gateway or GradioGateway(settings)
    slots = threading.BoundedSemaphore(settings.concurrency)
    executor = concurrent.futures.ThreadPoolExecutor(
        max_workers=settings.concurrency, thread_name_prefix="web-demo"
    )
    active: set[Control] = set()
    active_lock = threading.Lock()

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        yield
        with active_lock:
            for control in active:
                control.cancel()
        executor.shutdown(wait=False, cancel_futures=True)

    app = FastAPI(
        title="Isolated synthetic-document cloud demo",
        docs_url=None,
        redoc_url=None,
        openapi_url=None,
        lifespan=lifespan,
    )

    app.add_middleware(SameOriginMiddleware)

    @app.get("/demo-api/models")
    async def models() -> dict[str, Any]:
        return {
            "models": [
                {
                    "id": id_,
                    "label": settings.models[id_].label if id_ in settings.models else label,
                    "available": settings.enabled and id_ in settings.models,
                    **(
                        {"reason": "Cloud gateway is disabled"}
                        if not settings.enabled
                        else {"reason": UNAVAILABLE_REASONS[id_]}
                        if id_ not in settings.models
                        else {}
                    ),
                }
                for id_, label in CATALOG.items()
            ]
        }

    @app.post("/demo-api/extract")
    async def extract(request: Request) -> dict[str, Any]:
        if not settings.enabled:
            raise HTTPException(503, "Cloud gateway is disabled")
        if request.headers.get("x-cloud-consent", "").lower() != "true":
            raise HTTPException(400, "Explicit cloud consent is required")
        model = settings.models.get(request.headers.get("x-model-id", ""))
        if model is None:
            raise HTTPException(400, "Selected model is unavailable")
        mime = request.headers.get("content-type", "").split(";", 1)[0].strip().lower()
        if mime not in MIME_TYPES:
            raise HTTPException(415, "Only PNG, JPEG, and PDF uploads are supported")
        length = request.headers.get("content-length")
        if length is not None:
            if not length.isascii() or not length.isdecimal():
                raise HTTPException(400, "Invalid Content-Length")
            if len(length) > 12 or int(length) > MAX_UPLOAD:
                raise HTTPException(413, "Upload exceeds 10 MiB limit")
        if not slots.acquire(blocking=False):
            raise HTTPException(429, "Cloud demo is busy; retry later", headers={"Retry-After": "5"})

        started = time.monotonic()
        control = Control(started + settings.deadline_seconds)
        handed_off = False
        watcher: asyncio.Task | None = None
        with active_lock:
            active.add(control)

        def run(content: bytes) -> Any:
            try:
                return gateway.extract(model, content, mime, control)
            finally:
                slots.release()
                with active_lock:
                    active.discard(control)

        async def disconnected() -> None:
            while True:
                message = await request.receive()
                if message["type"] == "http.disconnect":
                    raise GatewayError(499, "Client disconnected")

        try:
            async with asyncio.timeout(settings.deadline_seconds):
                content = bytearray()
                async for chunk in request.stream():
                    if len(content) + len(chunk) > MAX_UPLOAD:
                        raise HTTPException(413, "Upload exceeds 10 MiB limit")
                    content.extend(chunk)
                if not content.startswith(MIME_TYPES[mime][1]):
                    raise HTTPException(415, "Upload signature does not match Content-Type")
                future = executor.submit(run, bytes(content))
                handed_off = True
                result_task = asyncio.wrap_future(future)
                result_task.add_done_callback(
                    lambda result: result.exception() if not result.cancelled() else None
                )
                watcher = asyncio.create_task(disconnected())
                done, _ = await asyncio.wait(
                    {result_task, watcher}, return_when=asyncio.FIRST_COMPLETED
                )
                if watcher in done:
                    watcher.result()
                data = result_task.result()
                text = text_from_result(data, model.text_pointer)
                return {
                    "model": model.id,
                    "text": text,
                    "data": data,
                    "elapsed_ms": round((time.monotonic() - started) * 1000),
                }
        except TimeoutError:
            raise HTTPException(504, "Cloud request deadline exceeded") from None
        except GatewayError as exc:
            raise HTTPException(exc.status, exc.message) from None
        except ClientDisconnect:
            raise HTTPException(499, "Client disconnected") from None
        finally:
            control.cancel()
            if watcher is not None:
                watcher.cancel()
                with contextlib.suppress(asyncio.CancelledError, GatewayError):
                    await watcher
            if not handed_off:
                slots.release()
                with active_lock:
                    active.discard(control)

    if settings.static_dir:
        directory = settings.static_dir.resolve()
        # Never serve the repository root, source tree, or arbitrary local files.
        root = Path(__file__).resolve().parent
        standalone = (
            root in directory.parents
            and directory.name == "dist"
            and (directory / "index.html").is_file()
        )
        studio_demo = (
            directory == root.parent / "studio" / "dist-demo"
            and (directory / "demo" / "index.html").is_file()
        )
        if not standalone and not studio_demo:
            raise ValueError(
                "Static directory must be a built web_demo/**/dist or studio/dist-demo"
            )

        @app.get("/")
        async def landing() -> RedirectResponse:
            return RedirectResponse("/demo/index.html" if studio_demo else "/demo/")

        # The Studio's separate multi-page demo build has demo/index.html and
        # root-level assets; only this build (never studio/dist) may mount at /.
        app.mount(
            "/" if studio_demo else "/demo",
            StaticFiles(directory=directory, html=True, follow_symlink=False),
            name="demo",
        )

    return app


app = create_app()
