"""Check the constructed set against its manifest, reading only the built PDFs.

For every case: the images PyMuPDF finds on the page must cover each manifest box (or none for a
control), no visible digital word may sit inside a box, and Tesseract on the rendered box must find
text for "ocr" boxes and next to none for "none" boxes. A "text_layer" box must carry invisible text.

usage: python tools/verify_set.py <outdir>
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import fitz

MIN_OCR_WORDS = 10   # an "ocr" box must read as at least this many words,
MIN_OCR_SHARE = 0.3  # or, when the source has few words there, this share of them
MAX_NONE_WORDS = 2   # a "none" box may read as noise, but no more than this
MIN_CONF = 60        # Tesseract invents words from scanner noise; count only confident ones


def ocr_words(page, box):
    pix = page.get_pixmap(dpi=200, clip=fitz.Rect(box), colorspace=fitz.csGRAY)
    with tempfile.TemporaryDirectory() as tmp:
        png = Path(tmp) / "b.png"
        pix.save(png)
        tsv = subprocess.run(["tesseract", str(png), "-", "-l", "eng", "--psm", "6", "tsv"],
                             capture_output=True, text=True, encoding="utf-8", errors="replace").stdout
    words = []
    for row in tsv.splitlines()[1:]:
        f = row.split("	")
        if len(f) == 12 and float(f[10]) >= MIN_CONF and sum(c.isalpha() for c in f[11]) >= 3:
            words.append(f[11])
    return words


def image_cover(page, box):
    """Fraction of box covered by the union of the page's image boxes (on a 2 pt grid)."""
    b = fitz.Rect(box)
    rects = [fitz.Rect(i["bbox"]) & b for i in page.get_image_info()]
    rects = [r for r in rects if not r.is_empty]
    if not rects or b.is_empty:
        return 0.0
    hit = tot = 0
    y = b.y0 + 1
    while y < b.y1:
        x = b.x0 + 1
        while x < b.x1:
            tot += 1
            hit += any(r.contains(fitz.Point(x, y)) for r in rects)
            x += 2
        y += 2
    return hit / tot if tot else 0.0


def main():
    out = Path(sys.argv[1])
    m = json.loads((out / "manifest.json").read_text(encoding="utf-8"))
    bad = []
    for c in m["items"]:
        doc = fitz.open(out / c["file"])
        page = doc[0]
        imgs = page.get_image_info()
        problems = []
        if c["kind"] == "control" and imgs:
            problems.append(f"control has {len(imgs)} images")
        if c["kind"] == "full_scan" and "words" not in c:
            problems.append("manifest lacks the source word count")
        for reg in c["regions"]:
            box = fitz.Rect(reg["box"])
            if c["kind"] != "background" and reg["expect"] != "text_layer":
                cov = image_cover(page, box)
                if cov < 0.95:
                    problems.append(f"image covers {cov:.2f} of box")
                inside = [w for w in page.get_text("words") if fitz.Rect(w[:4]).intersects(box)]
                if c["kind"] != "full_scan" and inside:
                    problems.append(f"{len(inside)} digital words in box")
            if reg["expect"] == "ocr":
                n = len(ocr_words(page, box))
                need = min(MIN_OCR_WORDS, MIN_OCR_SHARE * c.get("words", MIN_OCR_WORDS / MIN_OCR_SHARE))
                if n < need:
                    problems.append(f"OCR read {n} words")
            elif reg["expect"] == "none" and c["kind"] != "background":
                n = len(ocr_words(page, box))
                if n > MAX_NONE_WORDS:
                    problems.append(f"no-text box read as {n} words")
            elif reg["expect"] == "text_layer":
                hidden = page.get_text("words", clip=box)
                if len(hidden) < MIN_OCR_WORDS:
                    problems.append(f"text layer has {len(hidden)} words")
        if problems:
            bad.append((c["id"], c["kind"], c.get("wrap"), problems))
    for b in bad:
        print(*b)
    print(f"{len(m['items']) - len(bad)} of {len(m['items'])} cases check out")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
