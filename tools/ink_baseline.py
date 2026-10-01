"""Baseline for the ink test: the same check (tools/ink_check.py) on a map built from another
reader's boxes instead of regions-cli's, so the page map's coverage has something to stand against.

  pymupdf     the calls people reach for: get_text("words"), get_image_info(), get_drawings()
              (grown by half the line width), annotations and widgets
  bboxlog     PyMuPDF's get_bboxlog(): the box of every paint operation MuPDF makes, invisible text
              left out. The judge renders with MuPDF too, so this is the reader with home advantage.
  pdfplumber  pdfminer.six underneath, independent of MuPDF: chars (real spaces left out), rects, lines
              and curves (grown by half the line width), images, annotations; moved onto the crop
              box, which its own coordinates ignore

A baseline map has no flags, so its phantoms include marks regions-cli would flag (invisible,
offpage, white); coverage is the number to compare. reader_ms times the reader's own calls on each
page, not the render; it isn't comparable with regions-cli's time, which includes parsing the file.

usage:
  python tools/ink_baseline.py <reader> constructed [split=tune] [--worst=N] [--out=file.jsonl]
  python tools/ink_baseline.py <reader> real <labels.jsonl> <folder of PDFs> [--worst=N] [--out=file.jsonl]

Tuning data only: held-out sets are refused, as there's nothing to freeze here.
"""
import json
import statistics
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

import fitz

from ink_check import DPI, INK_LEVEL, KINDS, MARGIN_PT, check, pages_constructed, pages_real

READERS = ("pymupdf", "bboxlog", "pdfplumber")


def box(x0, y0, x1, y1):
    return {"x0": x0, "y0": y0, "x1": x1, "y1": y1, "reasons": []}


def turned(page, r, grow=0.0):
    """A PyMuPDF box (unrotated page) as the render shows it, grown by grow points."""
    r = fitz.Rect(r) * page.rotation_matrix
    return box(r.x0 - grow, r.y0 - grow, r.x1 + grow, r.y1 + grow)


def map_pymupdf(page, _plumber):
    words = [turned(page, w[:4]) for w in page.get_text("words")]
    images = [turned(page, i["bbox"]) for i in page.get_image_info()]
    paths = [turned(page, d["rect"], (d.get("width") or 0) / 2) for d in page.get_drawings()]
    annots = [turned(page, a.rect) for a in page.annots() or []] + [turned(page, w.rect) for w in page.widgets() or []]
    return words, images, paths, annots


def map_bboxlog(page, _plumber):
    words, images, paths = [], [], []
    for kind, r in page.get_bboxlog():
        if kind == "ignore-text":
            continue
        (words if kind.endswith("-text") else images if kind == "fill-image" else paths).append(turned(page, r))
    return words, images, paths, []


def map_pdfplumber(page, p):
    """pdfplumber's boxes are top-left based on the turned media box, crop box or not (003053 p9:
    crop box 37 pt in, /Rotate 90, every box 37 pt off), so they move by the crop box's corner on
    the turned media box."""
    turn = fitz.Matrix(page.rotation)
    r = page.mediabox * turn
    ox, oy = (page.cropbox * turn * fitz.Matrix(1, 0, 0, 1, -r.x0, -r.y0)).top_left

    def b(o, grow=0.0):
        return box(o["x0"] - ox - grow, o["top"] - oy - grow, o["x1"] - ox + grow, o["bottom"] - oy + grow)

    # only real spaces: str.isspace() is also true for control codes, which garbled fonts draw (003437)
    words = [b(c) for c in p.chars if c["text"] not in (" ", " ")]
    images = [b(i) for i in p.images]
    paths = [b(o, (o.get("linewidth") or 0) / 2) for o in p.rects + p.lines + p.curves]
    annots = [b(a) for a in p.annots]
    return words, images, paths, annots


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opt = dict(a[2:].split("=", 1) for a in sys.argv[1:] if a.startswith("--") and "=" in a)
    worst = int(opt.get("worst", 10))
    if len(args) < 2 or args[0] not in READERS or args[1] not in ("constructed", "real"):
        sys.exit(__doc__)
    reader = args[0]
    if args[1] == "constructed":
        split = args[2] if len(args) > 2 else "tune"
        if split != "tune":
            sys.exit("tuning data only")
        pages = pages_constructed(split)
    else:
        if "004" in Path(args[2]).name or Path(args[3]).name == "004":
            sys.exit("tuning data only")
        pages = pages_real(args[2], args[3])
    build = {"pymupdf": map_pymupdf, "bboxlog": map_bboxlog, "pdfplumber": map_pdfplumber}[reader]
    if reader == "pdfplumber":
        import pdfplumber

    by_file = defaultdict(list)
    for path, pno, group in pages:
        by_file[path].append((pno, group))
    rows = []
    for path, wanted in by_file.items():
        try:
            doc = fitz.open(path)
            plumber = pdfplumber.open(path) if reader == "pdfplumber" else None
        except Exception as e:
            rows += [{"file": path.name, "page": pno, "group": group, "error": str(e)} for pno, group in wanted]
            continue
        for pno, group in wanted:
            try:
                page = doc[pno]
                t = time.perf_counter()
                words, images, paths, annots = build(page, plumber.pages[pno] if plumber else None)
                ms = 1000 * (time.perf_counter() - t)
                pmap = {"regions": images, "words": words, "paths": paths, "annots": [dict(a, appearance=True) for a in annots]}
                rows.append({"file": path.name, "page": pno, "group": group, "reader_ms": round(ms, 3), **check(page, pmap)})
            except Exception as e:
                rows.append({"file": path.name, "page": pno, "group": group, "error": f"{type(e).__name__}: {e}"})
            if plumber:
                plumber.pages[pno].close()
        doc.close()
        if plumber:
            plumber.close()

    ok = [r for r in rows if "error" not in r]
    print(f"{reader}: {len(rows)} pages, {len(rows) - len(ok)} errors, {DPI} dpi, ink < {INK_LEVEL}, margin {MARGIN_PT} pt")
    c = [r["coverage"] for r in ok]
    print(f"{'pages':>5} {'100%':>6} {'>=99.9%':>8} {'>=99.5%':>8}   median  worst")
    print(f"{len(c):>5} {sum(x == 1 for x in c):>6} {sum(x >= 0.999 for x in c):>8} {sum(x >= 0.995 for x in c):>8}"
          f"   {100 * statistics.median(c):6.2f}%  {100 * min(c):6.2f}%")
    ink = sum(r["ink"] for r in ok)
    miss = Counter()
    for r in ok:
        miss.update(r["missed"])
    print(f"missed ink, share of all ink ({ink} px): " + ", ".join(f"{k} {100 * miss[k] / max(ink, 1):.3f}%" for k in KINDS))
    print("pages missing each kind: " + ", ".join(f"{k} {sum(1 for r in ok if r['missed'].get(k))}" for k in KINDS))
    ph = Counter()
    for r in ok:
        ph.update(r["phantoms"])
    print(f"phantoms (no flags, so invisible, offpage and white marks count): images {ph['image']}, words {ph['word']}, paths {ph['path']}, annots {ph['annot']}")
    ms = [r["reader_ms"] for r in ok]
    print(f"reader time a page: median {statistics.median(ms):.2f} ms, total {sum(ms) / 1000:.1f} s")
    print(f"worst {worst}:")
    for r in sorted(ok, key=lambda r: r["coverage"])[:worst]:
        print(f"  {r['file']} p{r['page']} {r['group']} {100 * r['coverage']:.2f}% missed {r['missed']}")
    for r in rows:
        if "error" in r:
            print(f"  error {r['file']} p{r['page']}: {r['error']}")
    if "out" in opt:
        Path(opt["out"]).write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
