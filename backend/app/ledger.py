"""Process-safe, atomic failure quarantine; contains no text or exception messages."""
import errno
import hashlib
import json
import os
import time
from contextlib import contextmanager
from pathlib import Path

from .config import private_dir


WINDOWS = os.name == "nt"
if WINDOWS:
    import msvcrt
else:
    import fcntl


@contextmanager
def _process_lock(lock):
    fd = lock.fileno()
    if WINDOWS:
        # Byte-range locks may extend beyond EOF; never truncate the shared lock file.
        while True:
            os.lseek(fd, 0, os.SEEK_SET)
            try:
                msvcrt.locking(fd, msvcrt.LK_NBLCK, 1)
                break
            except OSError as error:
                if error.errno not in (errno.EACCES, errno.EAGAIN, errno.EDEADLK):
                    raise
                time.sleep(.05)
    else:
        fcntl.flock(fd, fcntl.LOCK_EX)
    try:
        yield
    finally:
        if WINDOWS:
            os.lseek(fd, 0, os.SEEK_SET)
            msvcrt.locking(fd, msvcrt.LK_UNLCK, 1)
        else:
            fcntl.flock(fd, fcntl.LOCK_UN)


def fingerprint(parts):
    return hashlib.sha256(json.dumps(parts, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


class Ledger:
    def __init__(self, directory):
        self.directory = private_dir(Path(directory) / "logs")
        self.path = self.directory / "issues_ledger.json"
        self.lock = self.directory / "issues_ledger.lock"

    @contextmanager
    def locked(self):
        fd = os.open(self.lock, os.O_CREAT | os.O_RDWR, 0o600)
        with os.fdopen(fd, "r+b") as lock, _process_lock(lock):
            data = json.loads(self.path.read_text()) if self.path.exists() else {"version": 1, "issues": {}, "resets": []}
            yield data
            part = self.directory / f".ledger-{os.getpid()}.part"
            fd = os.open(part, os.O_CREAT | os.O_TRUNC | os.O_WRONLY, 0o600)
            with os.fdopen(fd, "w") as output:
                json.dump(data, output, sort_keys=True)
                output.flush()
                os.fsync(output.fileno())
            os.replace(part, self.path)
            if not WINDOWS:
                dirfd = os.open(self.directory, os.O_DIRECTORY)
                try:
                    os.fsync(dirfd)
                finally:
                    os.close(dirfd)

    def blocked(self, key):
        with self.locked() as data:
            return key in data["issues"]

    def fail(self, key, code):
        allowed = {"model_unavailable", "artifact_invalid", "dependency_unavailable",
                   "device_unavailable", "inference_failed", "deadline_exceeded"}
        if code not in allowed:
            code = "inference_failed"
        with self.locked() as data:
            old = data["issues"].get(key, {})
            data["issues"][key] = {"code": code, "count": old.get("count", 0) + 1,
                                   "at": time.time()}

    def reset(self, actor):
        with self.locked() as data:
            data["issues"] = {}
            data["resets"].append({"actor_digest": fingerprint(actor), "at": time.time()})
            data["resets"] = data["resets"][-1000:]
