"""Cross-platform durable ledger locking, including native Windows CI coverage."""
import errno
import json
import multiprocessing
import os
from pathlib import Path
import shutil
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import uuid

from backend.app import ledger


ROOT = Path(__file__).resolve().parents[1]


def add_failures(directory, count):
    store = ledger.Ledger(directory)
    for _ in range(count):
        store.fail("shared-fingerprint", "model_unavailable")


def hold_lock(path, acquired):
    fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
    with os.fdopen(fd, "r+b") as stream, ledger._process_lock(stream):
        acquired.set()
        time.sleep(30)


class CrossPlatformLedgerTests(unittest.TestCase):
    def setUp(self):
        self.path = ROOT / "var" / "windows-ledger-unit" / uuid.uuid4().hex
        self.path.mkdir(parents=True)

    def tearDown(self):
        shutil.rmtree(self.path)

    def test_process_contention_has_no_lost_updates_and_atomic_json(self):
        context = multiprocessing.get_context("spawn")
        processes = [context.Process(target=add_failures, args=(str(self.path), 10)) for _ in range(4)]
        try:
            for process in processes:
                process.start()
            for process in processes:
                process.join(30)
                self.assertEqual(process.exitcode, 0)
        finally:
            for process in processes:
                if process.is_alive():
                    process.terminate()
                    process.join(10)
        store = ledger.Ledger(self.path)
        self.assertTrue(store.blocked("shared-fingerprint"))
        contents = json.loads(store.path.read_text())
        self.assertEqual(contents["issues"]["shared-fingerprint"]["count"], 40)
        store.reset("operator")
        self.assertFalse(store.blocked("shared-fingerprint"))
        self.assertEqual(len(json.loads(store.path.read_text())["resets"]), 1)
        self.assertFalse(list(store.directory.glob(".ledger-*.part")))

    def test_exception_rolls_back_and_releases_lock(self):
        store = ledger.Ledger(self.path)
        store.fail("original", "model_unavailable")
        before = store.path.read_bytes()
        with self.assertRaisesRegex(RuntimeError, "abort"):
            with store.locked() as data:
                data["issues"].clear()
                raise RuntimeError("abort")
        self.assertEqual(store.path.read_bytes(), before)
        context = multiprocessing.get_context("spawn")
        process = context.Process(target=add_failures, args=(str(self.path), 1))
        process.start()
        try:
            process.join(15)
            self.assertEqual(process.exitcode, 0)
        finally:
            if process.is_alive():
                process.terminate()
                process.join(10)
        self.assertTrue(store.blocked("original"))
        self.assertTrue(store.blocked("shared-fingerprint"))

    def test_operating_system_releases_lock_when_holder_dies(self):
        context = multiprocessing.get_context("spawn")
        acquired = context.Event()
        store = ledger.Ledger(self.path)
        holder = context.Process(target=hold_lock, args=(str(store.lock), acquired))
        holder.start()
        try:
            self.assertTrue(acquired.wait(15))
        finally:
            holder.terminate()
            holder.join(10)
            if holder.is_alive():
                holder.kill()
                holder.join(10)
        writer = context.Process(target=add_failures, args=(str(self.path), 1))
        writer.start()
        try:
            writer.join(15)
            self.assertEqual(writer.exitcode, 0)
        finally:
            if writer.is_alive():
                writer.terminate()
                writer.join(10)
        self.assertTrue(store.blocked("shared-fingerprint"))

    def test_windows_lock_retries_contention_and_always_unlocks_same_byte(self):
        locking = Mock(side_effect=[OSError(errno.EACCES, "busy"),
                                    OSError(errno.EDEADLK, "busy"), None, None])
        api = SimpleNamespace(locking=locking, LK_NBLCK=2, LK_UNLCK=0)
        path = self.path / "lock"
        with (path.open("w+b") as stream,
              patch.object(ledger, "WINDOWS", True),
              patch.object(ledger, "msvcrt", api, create=True),
              patch.object(ledger.time, "sleep") as sleep):
            with self.assertRaisesRegex(RuntimeError, "test"):
                with ledger._process_lock(stream):
                    stream.seek(10)
                    raise RuntimeError("test")
            self.assertEqual(sleep.call_count, 2)
            self.assertEqual(locking.call_args_list[-1].args, (stream.fileno(), api.LK_UNLCK, 1))
            self.assertEqual(stream.tell(), 0)
            self.assertEqual(locking.call_count, 4)

    def test_windows_lock_does_not_ignore_non_contention_failures(self):
        api = SimpleNamespace(locking=Mock(side_effect=OSError(errno.EBADF, "invalid")),
                              LK_NBLCK=2, LK_UNLCK=0)
        with ((self.path / "lock").open("w+b") as stream,
              patch.object(ledger, "WINDOWS", True),
              patch.object(ledger, "msvcrt", api, create=True),
              patch.object(ledger.time, "sleep") as sleep):
            with self.assertRaises(OSError) as raised:
                with ledger._process_lock(stream):
                    self.fail("Lock failure was silently ignored")
            self.assertEqual(raised.exception.errno, errno.EBADF)
            sleep.assert_not_called()
            self.assertEqual(api.locking.call_count, 1)

    def test_windows_atomic_write_flushes_file_without_posix_directory_fsync(self):
        api = SimpleNamespace(locking=Mock(), LK_NBLCK=2, LK_UNLCK=0)
        store = ledger.Ledger(self.path)
        with (patch.object(ledger, "WINDOWS", True),
              patch.object(ledger, "msvcrt", api, create=True),
              patch.object(ledger.os, "fsync", wraps=os.fsync) as fsync):
            store.fail("shared-fingerprint", "model_unavailable")
            fsync.assert_called_once()
        self.assertEqual(json.loads(store.path.read_text())["issues"]["shared-fingerprint"]["count"], 1)
        self.assertEqual(api.locking.call_count, 2)


if __name__ == "__main__":
    unittest.main()
