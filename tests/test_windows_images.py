"""Windows decoder boundary compatibility without claiming POSIX resource parity."""
import builtins
import io
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, call, patch

from PIL import Image

from backend.app import images


def png():
    output = io.BytesIO()
    Image.new("RGB", (16, 12), "white").save(output, format="PNG")
    return output.getvalue()


class WindowsDecodeTests(unittest.TestCase):
    def test_windows_decode_never_imports_unix_resource(self):
        raw = png()
        pipe = Mock()
        original = builtins.__import__

        def guarded_import(name, *args, **kwargs):
            if name == "resource":
                raise AssertionError("Windows attempted a Unix resource import")
            return original(name, *args, **kwargs)

        with (patch.object(images.os, "name", "nt"),
              patch("builtins.__import__", side_effect=guarded_import)):
            images._decode_child(pipe, (raw, "image/png", 1000, 150))
        status, result = pipe.send.call_args.args[0]
        self.assertEqual(status, "ok")
        self.assertEqual(result[1:], (16, 12))
        pipe.close.assert_called_once()

    def test_posix_decode_keeps_both_hard_limits(self):
        raw = png()
        pipe = Mock()
        resource = SimpleNamespace(RLIMIT_AS=1, RLIMIT_CPU=2, setrlimit=Mock())
        with (patch.object(images.os, "name", "posix"),
              patch.dict(sys.modules, {"resource": resource})):
            images._decode_child(pipe, (raw, "image/png", 1000, 150))
        self.assertEqual(resource.setrlimit.call_args_list, [
            call(resource.RLIMIT_AS, (1536 * 1024**2, 1536 * 1024**2)),
            call(resource.RLIMIT_CPU, (15, 15)),
        ])
        self.assertEqual(pipe.send.call_args.args[0][0], "ok")
        pipe.close.assert_called_once()

    def test_windows_decode_keeps_pixel_and_signature_rejections(self):
        raw = png()
        for media, max_pixels in (("image/png", 100), ("image/jpeg", 1000)):
            with self.subTest(media=media, max_pixels=max_pixels):
                pipe = Mock()
                with patch.object(images.os, "name", "nt"):
                    images._decode_child(pipe, (raw, media, max_pixels, 150))
                pipe.send.assert_called_once_with(("error", "invalid_or_oversized_image"))
                pipe.close.assert_called_once()

    def test_real_spawned_decoder_keeps_geometry_and_rejects_oversized_input(self):
        raw, width, height = images.bounded_canonical(png(), "image/png", 1000, 150)
        self.assertEqual((width, height), (16, 12))
        self.assertTrue(raw.startswith(b"\x89PNG\r\n\x1a\n"))
        with self.assertRaises(images.ImageRejected):
            images.bounded_canonical(png(), "image/png", 100, 150)

    def test_parent_deadline_still_terminates_decoder(self):
        parent, child, process = Mock(), Mock(), Mock()
        parent.poll.return_value = False
        process.is_alive.side_effect = [True, False]
        context = Mock()
        context.Pipe.return_value = parent, child
        context.Process.return_value = process
        with patch.object(images.multiprocessing, "get_context", return_value=context):
            with self.assertRaisesRegex(images.ImageRejected, "image_decode_deadline"):
                images.bounded_canonical(b"input", "image/png", 1000, 150)
        parent.poll.assert_called_once_with(20)
        process.terminate.assert_called_once()
        process.join.assert_called_once_with(5)
        parent.close.assert_called_once()
