"""Label each image by its own pixels with Tesseract, for scoring the kind layer's has-text (D87).

tools/select_real.py labels a placement's box as rendered, so marks drawn over an image count. Here
each labelled placement's image object is decoded on its own (PyMuPDF), scaled to the size its box
has at DPI (so text is as large as the region labels saw it), and read with the same rules: a word
counts at confidence MIN_CONF or more with 3 letters or 2 digits (D67); TEXT_WORDS or more words is
text, none is none, in between is unsure. One line per (file, image object), in the order of the
region labels. Resumable: lines already in the output are skipped, so a stopped run can go on.

usage: python tools/label_images.py <pdf dir> <region labels.jsonl> <out.jsonl>
"""
import json
import sys
from pathlib import Path

import fitz

sys.path.insert(0, str(Path(__file__).parent))
from select_real import DPI, MIN_CONF, TEXT_WORDS, ocr_words  # noqa: E402

MAX_SIDE = 4000  # pixels; a larger scaled image is shrunk to this on its long side


def image_pixmap(doc, xref, box):
    pix = fitz.Pixmap(doc, xref)
    if pix.n - pix.alpha == 0:
        # a stencil mask comes back as alpha alone: painted (opaque) is black ink on white
        grey = bytes(255 - a for a in pix.samples)
        pix = fitz.Pixmap(fitz.csGRAY, pix.width, pix.height, grey, 0)
    elif pix.alpha:
        pix = fitz.Pixmap(pix, 0)
    # anything but plain grey or RGB (CMYK, Separation, Indexed...) through RGB, which PNG can hold
    if pix.colorspace is not None and pix.colorspace.name not in ("DeviceGray", "DeviceRGB"):
        pix = fitz.Pixmap(fitz.csRGB, pix)
    # the placement's size at DPI
    w = max(1, round((box[2] - box[0]) * DPI / 72))
    h = max(1, round((box[3] - box[1]) * DPI / 72))
    s = min(1.0, MAX_SIDE / max(w, h))
    w, h = max(1, round(w * s)), max(1, round(h * s))
    if (w, h) != (pix.width, pix.height):
        pix = fitz.Pixmap(pix, w, h, None)
    return pix


def main():
    src, labels, out = sys.argv[1:4]
    done = set()
    outp = Path(out)
    if outp.exists():
        for l in outp.read_text(encoding="utf-8").splitlines():
            if l.strip():
                j = json.loads(l)
                done.add((j["file"], j["xref"]))
    rows = [json.loads(l) for l in Path(labels).read_text(encoding="utf-8").splitlines() if l.strip()]
    todo = []
    for r in rows:
        for g in r["regions"]:
            key = (r["file"], g.get("xref"))
            if g.get("xref") and key not in done and key not in [t[0] for t in todo[-50:]]:
                todo.append((key, g["box"]))
    todo = list(dict((k, b) for k, b in todo).items())
    print(f"{len(done)} done, {len(todo)} to label", flush=True)
    with open(outp, "a", encoding="utf-8") as f:
        docs = {}
        for i, ((name, xref), box) in enumerate(todo):
            doc = docs.get(name) or docs.setdefault(name, fitz.open(Path(src) / name))
            if len(docs) > 4:
                for k in list(docs)[:-4]:
                    docs.pop(k).close()
            try:
                words = ocr_words(image_pixmap(doc, xref, box))
                n = len(words)
                label = "text" if n >= TEXT_WORDS else "none" if n == 0 else "unsure"
                rec = {"file": name, "xref": xref, "ocr_words": n, "label": label, "sample": words[:8]}
            except Exception as e:
                rec = {"file": name, "xref": xref, "error": f"{type(e).__name__}: {e}"[:120]}
            f.write(json.dumps(rec) + "\n")
            f.flush()
            if i % 100 == 0:
                print(f"  {i}/{len(todo)} {name}", flush=True)
    print("done", flush=True)


if __name__ == "__main__":
    main()
