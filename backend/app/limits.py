"""Bound small JSON request bodies before framework parsing."""
import asyncio
import json


class JSONLimit:
    def __init__(self, app):
        self.app = app

    async def __call__(self, scope, receive, send):
        if scope["type"] != "http" or scope["method"] not in ("POST", "PATCH", "PUT") or scope["path"] == "/api/v1/documents":
            await self.app(scope, receive, send)
            return
        body = bytearray()
        try:
            async with asyncio.timeout(30):
                while True:
                    message = await receive()
                    if message["type"] == "http.disconnect":
                        return
                    body.extend(message.get("body", b""))
                    if len(body) > 262144:
                        await self.reject(scope, send, 413, "json_body_limit")
                        return
                    if not message.get("more_body", False):
                        break
        except TimeoutError:
            await self.reject(scope, send, 408, "request_deadline")
            return

        sent = False

        async def replay():
            nonlocal sent
            if not sent:
                sent = True
                return {"type": "http.request", "body": bytes(body), "more_body": False}
            return await receive()

        await self.app(scope, replay, send)

    async def reject(self, scope, send, status, code):
        data = json.dumps({"code": code, "message": code.replace("_", " "),
                           "request_id": scope.get("state", {}).get("request_id", ""), "details": []}).encode()
        await send({"type": "http.response.start", "status": status,
                    "headers": [(b"content-type", b"application/json"), (b"cache-control", b"no-store")]})
        await send({"type": "http.response.body", "body": data})
