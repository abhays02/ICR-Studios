"""Canonical original-pixel crops; no automatic alignment or invented detector."""
import io
import multiprocessing
import os

from PIL import Image, ImageOps


class ImageRejected(Exception):
    pass


def canonical(raw, media, max_pixels, pdf_dpi):
    if media == "application/pdf":
        if not raw.startswith(b"%PDF-"):
            raise ImageRejected("mime_signature_mismatch")
        import pypdfium2 as pdfium
        with pdfium.PdfDocument(raw) as pdf:
            if len(pdf) != 1:
                raise ImageRejected("single_page_required")
            page = pdf[0]
            width, height = page.get_size()
            scale = pdf_dpi / 72
            if width <= 0 or height <= 0 or width * height * scale * scale > max_pixels:
                raise ImageRejected("pixel_limit")
            bitmap = page.render(scale=scale)
            image = bitmap.to_pil().copy()
            bitmap.close()
            page.close()
    else:
        if not ((media == "image/png" and raw.startswith(b"\x89PNG\r\n\x1a\n"))
                or (media == "image/jpeg" and raw.startswith(b"\xff\xd8\xff"))):
            raise ImageRejected("mime_signature_mismatch")
        image = Image.open(io.BytesIO(raw))
        if image.width * image.height > max_pixels or getattr(image, "n_frames", 1) != 1:
            raise ImageRejected("pixel_or_frame_limit")
        image.load()
        image = ImageOps.exif_transpose(image)
    if image.width * image.height > max_pixels:
        raise ImageRejected("pixel_limit")
    if image.mode in ("RGBA", "LA", "P") or "transparency" in image.info:
        rgba = image.convert("RGBA")
        image = Image.new("RGB", image.size, "white")
        image.paste(rgba, mask=rgba.getchannel("A"))
    else:
        image = image.convert("RGB")
    result = io.BytesIO()
    image.save(result, format="PNG")
    return result.getvalue(), image.width, image.height


def _decode_child(pipe, args):
    try:
        if os.name != "nt":
            import resource
            resource.setrlimit(resource.RLIMIT_AS, (1536 * 1024**2, 1536 * 1024**2))
            resource.setrlimit(resource.RLIMIT_CPU, (15, 15))
        # Windows retains the parent deadline and pixel/input caps, not POSIX hard limits.
        pipe.send(("ok", canonical(*args)))
    except Exception:
        pipe.send(("error", "invalid_or_oversized_image"))
    finally:
        pipe.close()


def bounded_canonical(raw, media, max_pixels, pdf_dpi):
    context = multiprocessing.get_context("spawn")
    parent, child = context.Pipe()
    process = context.Process(target=_decode_child, args=(child, (raw, media, max_pixels, pdf_dpi)))
    process.start()
    child.close()
    try:
        if not parent.poll(20):
            raise ImageRejected("image_decode_deadline")
        status, result = parent.recv()
        if status != "ok":
            raise ImageRejected(result)
        return result
    except EOFError:
        raise ImageRejected("image_decoder_failed") from None
    finally:
        if process.is_alive():
            process.terminate()
        process.join(5)
        if process.is_alive():
            process.kill()
            process.join()
        parent.close()


def crop(raw, box):
    with Image.open(io.BytesIO(raw)) as image:
        x0, y0, x1, y1 = box
        if not 0 <= x0 < x1 <= image.width or not 0 <= y0 < y1 <= image.height:
            raise ImageRejected("invalid_crop")
        output = io.BytesIO()
        image.crop(tuple(box)).save(output, format="PNG")
        return output.getvalue()
