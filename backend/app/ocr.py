"""Real local TrOCR only. Heavy libraries are never imported at API startup."""
import hashlib
import io
import json
import os
import re
from pathlib import Path

import yaml

MODEL_ID = "microsoft/trocr-base-handwritten"
CANDIDATES = [
    "AIDC-AI/OvisOCR2", "ATH-MaaS/OvisOCR2", "datalab-to/chandra-ocr-2",
    "ucaslcl/GOT-OCR2_0", "VikParuchuri/surya", "Riksarkivet/trocr-base-handwritten",
]
FORENSIC_PROMPT = (
    "Forensic Banking ICR Engine: Transcribe handwritten fields literally without spell-correcting names/addresses.\n"
    "- Comb-boxes (Account No/Phone/PIN): Enforce digits only (map visual O->0, I/l->1, S->5, B->8 inside numeric boxes).\n"
    "- Cross-check CAR (Amount in Words) vs LAR (Amount in Figures); flag mismatches.\n"
    "- Validate IFSC (11 chars), PAN (10 chars), Date (DD/MM/YYYY), and checkboxes (`[x]`/`[ ]`).\n"
    "- Return strict JSON: `doc_type`, `fields` (`value`, `conf`, `model_used`, `bbox`), `car_lar_match`, `anomalies`."
)


class ModelError(Exception):
    def __init__(self, code):
        self.code = code
        super().__init__(code)


def digest_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def verify_snapshot(directory):
    directory = Path(directory)
    if not (directory / "manifest.json").is_file():
        raise ModelError("model_unavailable")
    try:
        if (directory / "manifest.json").stat().st_size > 1048576:
            raise ValueError()
        manifest = json.loads((directory / "manifest.json").read_text())
        if (manifest["schema_version"] != 1 or manifest["model_id"] != MODEL_ID
                or not re.fullmatch(r"[0-9a-f]{40}", manifest["revision"])
                or not isinstance(manifest["license"], str) or not 1 <= len(manifest["license"]) <= 128
                or manifest["operator_approved"] is not True):
            raise ValueError()
        files = manifest["files"]
        if not isinstance(files, dict) or len(files) > 1000:
            raise ValueError()
        actual = {str(p.relative_to(directory)) for p in directory.rglob("*") if p.is_file()}
        if actual != set(files) | {"manifest.json"}:
            raise ValueError()
        if not any(f.endswith(".safetensors") for f in files) or not {"config.json", "README.md"} <= set(files):
            raise ValueError()
        for name, digest in files.items():
            path = directory / name
            if (Path(name).is_absolute() or ".." in Path(name).parts or path.is_symlink()
                    or path.resolve().parent != directory.resolve() or not re.fullmatch(r"[a-f0-9]{64}", digest)
                    or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", name)
                    or not (name.endswith((".json", ".txt", ".safetensors")) or name in ("README.md", "LICENSE"))
                    or digest_file(path) != digest):
                raise ValueError()
        config = json.loads((directory / "config.json").read_text())
        if config.get("model_type") != "vision-encoder-decoder" or "auto_map" in json.dumps(config):
            raise ValueError()
        if manifest["license_evidence"] != f"https://huggingface.co/{MODEL_ID}/blob/{manifest['revision']}/README.md":
            raise ValueError()
        card = (directory / "README.md").read_text()
        if not card.startswith("---\n"):
            raise ValueError()
        metadata = yaml.safe_load(card.split("---", 2)[1])
        if metadata.get("license") != manifest["license"]:
            raise ValueError()
        return manifest
    except ModelError:
        raise
    except Exception:
        raise ModelError("artifact_invalid") from None


def model_status(directory):
    # Public readiness must not hash gigabytes or import frameworks on each poll.
    path = Path(directory) / "manifest.json"
    if not path.is_file():
        return {"model_id": MODEL_ID, "status": "model_unavailable", "inference_tested": False}
    try:
        if path.stat().st_size > 1048576:
            raise ValueError()
        manifest = json.loads(path.read_text())
        if manifest["model_id"] != MODEL_ID or not re.fullmatch(r"[a-f0-9]{40}", manifest["revision"]):
            raise ValueError()
        return {"model_id": MODEL_ID, "status": "artifact_present_not_loaded",
                "revision": manifest["revision"], "inference_tested": False,
                "checksum_verification": "required_on_load"}
    except Exception:
        return {"model_id": MODEL_ID, "status": "artifact_invalid", "inference_tested": False}


def choose_device(torch, requested):
    if requested == "auto":
        if torch.cuda.is_available():
            return "cuda"
        if hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
            return "mps"
        return "cpu"
    if requested == "cuda" and not torch.cuda.is_available():
        raise ModelError("device_unavailable")
    if requested == "mps" and not (hasattr(torch.backends, "mps") and torch.backends.mps.is_available()):
        raise ModelError("device_unavailable")
    return requested


class TrOCR:
    def __init__(self, directory, device):
        self.directory, self.requested = directory, device
        self.model = self.processor = None

    def load(self):
        verify_snapshot(self.directory)
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
        try:
            import torch
            from transformers import TrOCRProcessor, VisionEncoderDecoderModel
        except ImportError:
            raise ModelError("dependency_unavailable") from None
        self.device = choose_device(torch, self.requested)
        torch.set_num_threads(max(1, min(4, os.cpu_count() or 1)))
        try:
            self.processor = TrOCRProcessor.from_pretrained(
                self.directory, local_files_only=True, trust_remote_code=False)
            self.model = VisionEncoderDecoderModel.from_pretrained(
                self.directory, local_files_only=True, trust_remote_code=False,
                use_safetensors=True, attn_implementation="eager", dtype=torch.float32)
            self.model.to(device=self.device, dtype=torch.float32).eval()
        except Exception:
            raise ModelError("artifact_invalid") from None

    def infer(self, raw, max_tokens):
        if self.model is None:
            self.load()
        import torch
        from PIL import Image
        with Image.open(io.BytesIO(raw)) as image, torch.inference_mode():
            pixels = self.processor(images=image.convert("RGB"), return_tensors="pt").pixel_values
            tokens = self.model.generate(pixels.to(self.device, dtype=torch.float32),
                                         max_new_tokens=max_tokens, do_sample=False, num_beams=1)
            text = self.processor.batch_decode(tokens, skip_special_tokens=True)[0]
            eos = self.model.generation_config.eos_token_id
            eos = eos if isinstance(eos, list) else [eos]
            truncated = tokens.shape[-1] - 1 >= max_tokens and int(tokens[0, -1]) not in eos
        if len(text) > 4096:
            raise ModelError("inference_failed")
        return {"raw_text": text, "confidence": None, "confidence_kind": "unavailable",
                "device": self.device, "dtype": "float32", "model_used": MODEL_ID, "truncated": truncated}

    def unload(self):
        self.model = self.processor = None
        import gc
        gc.collect()


def inference_child(pipe, directory, device):
    adapter = TrOCR(directory, device)
    try:
        while True:
            request = pipe.recv()
            if request is None:
                break
            try:
                pipe.send(("ok", adapter.infer(*request)))
            except ModelError as error:
                pipe.send(("error", error.code))
            except Exception:
                pipe.send(("error", "inference_failed"))
    except (EOFError, BrokenPipeError):
        pass
    finally:
        adapter.unload()
        pipe.close()
