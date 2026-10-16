"""Offline fixture integrity/geometry tests; no inference or network."""
import hashlib
import importlib.machinery
import importlib.util
import json
import shutil
import unittest
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"


def load(path):
    return json.loads(path.read_text())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class SampleTests(unittest.TestCase):
    def test_manifest_provenance_and_pngs(self):
        manifest = load(DATA / "samples/manifest.json")
        self.assertFalse(manifest["is_real_handwriting"])
        self.assertEqual(sha(ROOT / manifest["generator"]), manifest["generator_sha256"])
        self.assertEqual(len(manifest["samples"]), 6)
        self.assertLess(sum(p.stat().st_size for p in DATA.rglob("*") if p.is_file()),
                        8 * 1024 * 1024)
        for font in manifest["fonts"]:
            self.assertEqual(sha(DATA / "fonts" / font["file"]), font["sha256"])
            self.assertTrue(font["license"])
            self.assertTrue(font["source"].startswith("https://"))
        for sample in manifest["samples"]:
            path = DATA / "samples" / sample["image"]
            self.assertEqual(sha(path), sample["image_sha256"])
            self.assertEqual(sha(DATA / "samples" / sample["ground_truth"]),
                             sample["ground_truth_sha256"])
            with Image.open(path) as image:
                self.assertEqual(image.size, (1800, 2400))
                self.assertEqual(image.mode, "RGB")
                image.verify()

    def test_geometry_and_ink_containment(self):
        for sample in load(DATA / "samples/manifest.json")["samples"]:
            truth = load(DATA / "samples" / sample["ground_truth"])
            template = load(DATA / "samples" / sample["template"])
            self.assertEqual(set(truth["fields"]), {f["field_id"] for f in template["fields"]})
            matrix = truth["template_to_image"]
            for field in truth["fields"].values():
                roi = field["template_roi"]
                for bbox in [roi, field["original_bbox"]] + field["original_line_bboxes"]:
                    self.assertTrue(0 <= bbox[0] < bbox[2] <= 1800)
                    self.assertTrue(0 <= bbox[1] < bbox[3] <= 2400)
                for ink in field["template_ink_bboxes"]:
                    self.assertTrue(roi[0] <= ink[0] < ink[2] <= roi[2])
                    self.assertTrue(roi[1] <= ink[1] < ink[3] <= roi[3])
                    self.assertTrue(any(line[0] <= ink[0] < ink[2] <= line[2]
                                        and line[1] <= ink[1] < ink[3] <= line[3]
                                        for line in field["template_line_rois"]))
                corners = [(roi[0], roi[1]), (roi[2], roi[1]),
                           (roi[2], roi[3]), (roi[0], roi[3])]
                for (x, y), point in zip(corners, field["original_polygon"]):
                    self.assertAlmostEqual(point[0], matrix[0][0] * x + matrix[0][1] * y
                                           + matrix[0][2], places=5)
                    self.assertAlmostEqual(point[1], y, places=5)
        self.assertEqual(truth["fields"]["date_of_birth"]["value"], "1991-02-30")
        self.assertTrue(truth["alignment_required"])

    def test_reproducible_generation(self):
        loader = importlib.machinery.SourceFileLoader("sample_generator",
                                                     str(ROOT / "scripts/generate_samples"))
        spec = importlib.util.spec_from_loader(loader.name, loader)
        module = importlib.util.module_from_spec(spec)
        loader.exec_module(module)
        output = ROOT / ".sample-validation"
        self.assertFalse(output.exists(), "Refusing to overwrite an existing validation directory")
        try:
            module.generate(output, DATA / "fonts", 20261015)
            for directory in ["samples", "templates", "schemas"]:
                for generated in (output / directory).iterdir():
                    self.assertEqual(sha(generated), sha(DATA / directory / generated.name))
        finally:
            if output.exists():
                shutil.rmtree(output)

    def test_business_schema_and_edge_cases(self):
        for kind in ["car", "lar"]:
            schema = load(DATA / "schemas" / f"{kind}-v1.0.0.schema.json")
            self.assertFalse(schema["additionalProperties"])
            self.assertIn("full_name", schema["required"])
            self.assertEqual(schema["properties"]["signature_present"]["type"], ["boolean", "null"])
        lar = load(DATA / "samples/02-lar.ground_truth.json")
        self.assertEqual(lar["fields"]["tenure_months"]["raw_text"], "024")
        self.assertEqual(lar["fields"]["tenure_months"]["value"], 24)
        car = load(DATA / "samples/01-car.ground_truth.json")
        self.assertEqual(car["fields"]["tax_identifier"]["value"], "")
        self.assertEqual(len(car["fields"]["address"]["original_line_bboxes"]), 2)
        kyc = load(DATA / "samples/03-kyc.ground_truth.json")
        self.assertIs(kyc["fields"]["consent"]["value"], False)
        rtgs = load(DATA / "samples/04-rtgs.ground_truth.json")
        self.assertEqual(rtgs["fields"]["account_number"]["value"], "000000012345")


if __name__ == "__main__":
    unittest.main()
