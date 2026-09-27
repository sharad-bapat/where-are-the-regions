"""Contact sheets of a seeded sample of labelled real regions, for checking the Tesseract labels by eye.

Each tile is the region rendered with the page's digital text removed (what Tesseract saw), captioned
with its index and label. A hand verdict per index goes in a JSON file next to the sheets:
{"<file>#<page>#<n>": "text" | "none"}.

usage: python tools/sample_sheet.py <pdf dir> <regions.jsonl> <out dir> [per_label=25] [seed]
"""
import json
import random
import sys
from pathlib import Path

import fitz
from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).parent))
from select_real import textless  # noqa: E402

TILE = 300


def main():
    src, jsonl, outdir, *rest = sys.argv[1:]
    per = int(rest[0]) if rest else 25
    seed = int(rest[1]) if len(rest) > 1 else 20260927
    rows = [json.loads(l) for l in open(jsonl, encoding="utf-8")]
    items = [(r["file"], r["page"], n, reg) for r in rows for n, reg in enumerate(r["regions"])]
    rng = random.Random(seed)
    picked = []
    for label in ("text", "none", "unsure"):
        pool = [i for i in items if i[3]["label"] == label]
        picked += rng.sample(pool, min(per, len(pool)))
    out = Path(outdir)
    out.mkdir(parents=True, exist_ok=True)
    keys = []
    cols, rows_per = 5, 4
    for s in range(0, len(picked), cols * rows_per):
        sheet = Image.new("RGB", (cols * TILE, rows_per * (TILE + 20)), "white")
        d = ImageDraw.Draw(sheet)
        for k, (f, pno, n, reg) in enumerate(picked[s:s + cols * rows_per]):
            bare = textless(fitz.open(Path(src) / f), pno)[0]
            pix = bare.get_pixmap(dpi=110, clip=fitz.Rect(reg["box"]))
            img = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
            img.thumbnail((TILE - 6, TILE - 6))
            x, y = (k % cols) * TILE, (k // cols) * (TILE + 20)
            sheet.paste(img, (x + 3, y + 3))
            d.rectangle([x, y, x + TILE - 1, y + TILE - 1], outline="grey")
            idx = s + k
            d.text((x + 4, y + TILE + 3), f"{idx} {reg['label']} ({reg['ocr_words']}w)", fill="black")
            keys.append({"idx": idx, "key": f"{f}#{pno}#{n}", "label": reg["label"], "ocr_words": reg["ocr_words"]})
        sheet.save(out / f"sheet{s // (cols * rows_per):02d}.png")
    (out / "sample.json").write_text(json.dumps(keys, indent=1), encoding="utf-8")
    print(f"{len(picked)} regions on {-(-len(picked) // (cols * rows_per))} sheets in {out}")


if __name__ == "__main__":
    main()
