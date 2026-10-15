"""Context-bound AEAD; keys only enter from the process environment."""
import base64
import json
import os
from dataclasses import dataclass

from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from sqlalchemy import LargeBinary
from sqlalchemy.types import TypeDecorator


class IntegrityError(Exception):
    pass


@dataclass(frozen=True)
class Sealed:
    value: bytes


class Encrypted(TypeDecorator):
    impl = LargeBinary
    cache_ok = True

    def process_bind_param(self, value, dialect):
        if value is None:
            return None
        if not isinstance(value, Sealed):
            raise TypeError("Sensitive ORM/bulk writes must use context-bound AEAD")
        return value.value

    def process_result_value(self, value, dialect):
        return Sealed(value) if value is not None else None


class Keyring:
    def __init__(self, keys, active):
        if active not in keys or any(len(v) != 32 for v in keys.values()):
            raise ValueError("32-byte keyring and active key required")
        self.keys, self.active = keys, active

    @classmethod
    def from_env(cls):
        raw = json.loads(os.environ.get("ICR_KEYS", "{}"))
        return cls({k: base64.b64decode(v, validate=True) for k, v in raw.items()},
                   os.environ.get("ICR_ACTIVE_KEY", ""))

    def seal(self, value, owner, row_id, column="data"):
        header = {"v": 1, "kid": self.active, "alg": "AES-256-GCM"}
        aad = json.dumps([header, owner, row_id, column], separators=(",", ":")).encode()
        nonce = os.urandom(12)
        plain = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
        header["nonce"] = base64.b64encode(nonce).decode()
        header["ct"] = base64.b64encode(AESGCM(self.keys[self.active]).encrypt(nonce, plain, aad)).decode()
        return Sealed(json.dumps(header, separators=(",", ":")).encode())

    def open(self, sealed, owner, row_id, column="data"):
        try:
            envelope = json.loads(sealed.value)
            if envelope["v"] != 1 or envelope["alg"] != "AES-256-GCM":
                raise ValueError()
            header = {k: envelope[k] for k in ("v", "kid", "alg")}
            aad = json.dumps([header, owner, row_id, column], separators=(",", ":")).encode()
            plain = AESGCM(self.keys[envelope["kid"]]).decrypt(
                base64.b64decode(envelope["nonce"]), base64.b64decode(envelope["ct"]), aad)
            return json.loads(plain)
        except Exception:
            raise IntegrityError("encrypted_data_integrity") from None
