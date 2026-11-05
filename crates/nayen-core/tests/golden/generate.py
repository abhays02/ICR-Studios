#!/usr/bin/env python3
"""Regenerate the golden reference values pinned in `tests/golden_reference.rs`.

Requires the real reference implementation and OpenCV:

    pip install indra-ocr opencv-python-headless numpy

The pages built here must stay byte-identical to the Rust fixtures in
`tests/golden_reference.rs` (`page()` and `banner()`), or the comparison is
meaningless. Both are deterministic and noise-free for exactly that reason.

Run from the crate root:

    python3 tests/golden/generate.py

Prints a JSON object; diff it against the constants in the Rust file and update
them together.
"""
import json

import numpy as np
from indra_ocr.det_post import decode, page_to_input
from indra_ocr.rec_preproc import crop_line, is_dark_line, pad_batch, width_batches

H, W = 220, 400
BG, INK = 250, 25
LINE_ROWS = [(40, 62), (100, 122), (160, 182)]


def page():
    d = np.full((H, W), BG, np.uint8)
    for y0, y1 in LINE_ROWS:
        for x in range(24, W - 24, 7):
            d[y0 + 3:y1 - 3, x:x + 4] = INK
    return d


def banner():
    d = np.full((H, W), BG, np.uint8)
    d[30:90, :] = 28
    for x in range(40, 360, 9):
        d[45:75, x:x + 4] = 235
    return d


BOX = [20.0, 38.0, 380.0, 64.0]
BANNER_BOX = [20.0, 30.0, 380.0, 90.0]


def crop_stats(c):
    return {
        "h": int(c.shape[0]),
        "w": int(c.shape[1]),
        "mean": float(c.mean()),
        "max": float(c.max()),
        "min": float(c.min()),
    }


def main():
    p, b = page(), banner()
    g = {}

    ink, scale = page_to_input(p, 256)
    g["pti_256"] = {
        "h": int(ink.shape[0]), "w": int(ink.shape[1]),
        "scale": float(scale), "sum": float(ink.sum()), "mean": float(ink.mean()),
    }

    g["is_dark_normal"] = bool(is_dark_line(p, BOX))
    g["is_dark_banner"] = bool(is_dark_line(b, BANNER_BOX))

    g["crop_w3"] = crop_stats(crop_line(p, BOX, window=3, pad_y=0.2, pad_x=0.3))
    g["crop_w1"] = crop_stats(crop_line(p, BOX, window=1, pad_y=0.2, pad_x=0.3))
    g["crop_hw"] = crop_stats(crop_line(p, BOX, window=3, pad_y=0.35, pad_x=0.4))
    g["crop_banner"] = crop_stats(crop_line(b, BANNER_BOX, window=3, pad_y=0.2, pad_x=0.3))

    crops = [crop_line(p, [20.0, y0 - 2, 380.0, y1 + 2], window=3) for y0, y1 in LINE_ROWS]
    arr, widths = pad_batch(crops)
    g["pad_batch"] = {
        "shape": [int(v) for v in arr.shape],
        "widths": [int(w) for w in widths],
        "pad_zero": bool(np.all(arr[1, 0, :, widths[1]:] == 0)),
        "mean": float(arr.mean()),
    }

    g["wb_similar"] = [[int(i) for i in batch] for batch in width_batches([100, 102, 98, 101, 900, 910])]
    g["wb_maxbatch_n"] = len(width_batches([64] * 133))
    g["wb_area_sizes"] = [len(batch) for batch in width_batches([4096] * 64)]

    det_h, det_w = 24, 48

    def synthetic(logit):
        out = np.zeros((3, det_h, det_w), np.float32)
        for y0, y1 in [(6, 10), (16, 20)]:
            for y in range(y0, y1):
                for x in range(5, 40):
                    out[0, y, x] = logit
                    out[1, y, x] = ((y - y0) + 0.5) * 2.0
                    out[2, y, x] = ((y1 - 1 - y) + 0.5) * 2.0
        return out

    det = synthetic(5.0)
    g["decode_ref"] = [[round(float(v), 4) for v in r] for r in decode(det)]
    g["decode_grow"] = [
        [round(float(v), 4) for v in r]
        for r in decode(det, min_area=1, grow_y=0.12, grow_x=0.08)
    ]
    faint = synthetic(-0.4)
    g["decode_faint_ref_n"] = int(len(decode(faint)))
    g["decode_faint_lo_n"] = int(len(decode(faint, thresh=0.3, min_area=1)))

    print(json.dumps(g, indent=1))


if __name__ == "__main__":
    main()
