"""Build the marks set: one-page PDFs written by hand, so every mark's box is known by construction.

The first constructed set pastes images onto real contract pages, so only the pasted boxes are
known. This one draws everything itself, one mark per cell of a grid (so no two marks touch and each
is one region of the map):

  text       a line of words in a standard font (Helvetica, Times, Courier), some drawn invisible
  rule       a stroked line
  rect       a filled or stroked rectangle, some filled white
  circle     a filled circle (four curves)
  clipped    a rectangle cut by a clip; hidden when the clip misses it entirely
  shading    an axial shading painting its clip
  image      a small greyscale image XObject
  offpage    a rectangle placed off the page
  link       a link annotation (no appearance)
  field      a text field widget with an appearance that strokes its border

Truth boxes come from the geometry and Adobe's font metrics (widths and ascent/descent, via
reportlab), in points from the top left of the page as displayed. On pages with /Rotate 90 the
boxes are turned by PyMuPDF's rotation matrix, not by the parser's code. Stroked boxes grow by half
the line width.

The split (tune or heldout) is by a hash of the case id. The output is deterministic: rerunning
with the same seed writes byte-identical files.

usage: python tools/build_marks.py <outdir> [cases=200] [seed]
"""
import hashlib
import json
import random
import sys
from pathlib import Path

import fitz
from reportlab.pdfbase._fontdata import ascent_descent
from reportlab.pdfbase.pdfmetrics import stringWidth

SEED = 20260929
FONTS = [("F1", "Helvetica"), ("F2", "Times-Roman"), ("F3", "Courier")]
WORDS = "notice party agreement term confidential information shall means within days written consent".split()
KINDS = ["text"] * 5 + ["rule", "rect", "rect", "circle", "clipped", "shading", "image", "offpage", "link", "field"]
CELL_W, CELL_H, MARGIN = 180.0, 60.0, 36.0


def split_of(cid):
    return "heldout" if hashlib.sha256(cid.encode()).digest()[0] % 2 else "tune"


def pdf_bytes(width, height, rotate, content, annots, extra):
    """A one-page PDF with the standard fonts, one image, one shading and the given annotations."""
    objs = {}
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    fonts = b" ".join(b"/%s << /Type /Font /Subtype /Type1 /BaseFont /%s >>" % (k.encode(), n.encode()) for k, n in FONTS)
    annot_refs = b" ".join(b"%d 0 R" % n for n in range(20, 20 + len(annots)))
    objs[3] = (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %g %g] /Rotate %d /Contents 4 0 R "
               b"/Resources << /Font << %s >> /XObject << /Im 5 0 R >> /Shading << /Sh 6 0 R >> >> /Annots [%s] >>"
               % (width, height, rotate, fonts, annot_refs))
    objs[4] = b"<< /Length %d >>\nstream\n%s\nendstream" % (len(content), content)
    pix = bytes((x * 37 + y * 11) % 256 for y in range(8) for x in range(8))
    objs[5] = b"<< /Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 64 >>\nstream\n" + pix + b"\nendstream"
    objs[6] = b"<< /ShadingType 2 /ColorSpace /DeviceGray /Coords [0 0 1 0] /Function << /FunctionType 2 /Domain [0 1] /C0 [0.2] /C1 [0.8] /N 1 >> >>"
    for i, a in enumerate(annots):
        objs[20 + i] = a
    objs.update(extra)
    out = bytearray(b"%PDF-1.7\n")
    offsets = {}
    for n in sorted(objs):
        offsets[n] = len(out)
        out += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    top = max(objs) + 1
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % top
    for n in range(1, top):
        out += (b"%010d 00000 n \n" % offsets[n]) if n in offsets else b"0000000000 65535 f \n"
    out += b"trailer << /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (top, xref)
    return bytes(out)


def build_case(rng, i):
    cid = f"m{i:04d}"
    width, height = rng.choice([(612.0, 792.0), (595.0, 842.0)])
    rotate = 90 if rng.random() < 0.2 else 0
    ops, annots, extra, marks = [], [], {}, []
    cols = int((width - 2 * MARGIN) // CELL_W)
    rows = int((height - 2 * MARGIN) // CELL_H)
    cells = [(c, r) for r in range(rows) for c in range(cols)]
    rng.shuffle(cells)

    def mark(what, box, flags=(), **info):
        # box in unrotated user space (x0, y0, x1, y1 with y up); turned to the page later
        marks.append({"what": what, "user_box": [round(v, 4) for v in box], "flags": sorted(flags), **info})

    for c, r in cells[:rng.randint(6, 14)]:
        x, y = MARGIN + c * CELL_W, height - MARGIN - (r + 1) * CELL_H  # the cell's lower-left corner
        kind = rng.choice(KINDS)
        if kind == "text":
            key, font = rng.choice(FONTS)
            size = rng.choice([6, 8, 10, 12, 14, 18])
            words = " ".join(rng.choice(WORDS) for _ in range(rng.randint(1, 4)))
            while stringWidth(words, font, size) > CELL_W - 12 and " " in words:
                words = words.rsplit(" ", 1)[0]
            bx, by = x + 6, y + 20
            asc, desc = ascent_descent[font]
            invisible = rng.random() < 0.2
            ops.append(f"BT {'3 Tr ' if invisible else ''}/{key} {size} Tf {bx:g} {by:g} Td ({words}) Tj ET")
            mark("text", (bx, by + desc * size / 1000, bx + stringWidth(words, font, size), by + asc * size / 1000),
                 ["invisible"] if invisible else [], text=words, font=font, size=size)
        elif kind == "rule":
            w = rng.choice([0.5, 1, 2])
            x1, x2, yy = x + 6, x + CELL_W - 6, y + 30
            ops.append(f"q {w:g} w {x1:g} {yy:g} m {x2:g} {yy:g} l S Q")
            mark("vector", (x1, yy - w / 2, x2, yy + w / 2), ["stroke"])
        elif kind == "rect":
            rw, rh = rng.uniform(20, CELL_W - 20), rng.uniform(8, CELL_H - 16)
            x0, y0 = x + 8, y + 8
            if rng.random() < 0.5:
                white = rng.random() < 0.3
                ops.append(f"q {'1 1 1' if white else '0.2 0.4 0.6'} rg {x0:g} {y0:g} {rw:.2f} {rh:.2f} re f Q")
                mark("vector", (x0, y0, x0 + rw, y0 + round(rh, 2)), ["fill"] + (["white"] if white else []))
            else:
                w = rng.choice([0.5, 1, 2])
                ops.append(f"q {w:g} w {x0:g} {y0:g} {rw:.2f} {rh:.2f} re S Q")
                mark("vector", (x0 - w / 2, y0 - w / 2, x0 + round(rw, 2) + w / 2, y0 + round(rh, 2) + w / 2), ["stroke"])
        elif kind == "circle":
            rad = rng.uniform(8, 25)
            cx, cy, k = x + 40, y + 30, 0.5523 * rad
            ops.append(f"q 0 0 0 rg {cx + rad:.3f} {cy:.3f} m "
                       f"{cx + rad:.3f} {cy + k:.3f} {cx + k:.3f} {cy + rad:.3f} {cx:.3f} {cy + rad:.3f} c "
                       f"{cx - k:.3f} {cy + rad:.3f} {cx - rad:.3f} {cy + k:.3f} {cx - rad:.3f} {cy:.3f} c "
                       f"{cx - rad:.3f} {cy - k:.3f} {cx - k:.3f} {cy - rad:.3f} {cx:.3f} {cy - rad:.3f} c "
                       f"{cx + k:.3f} {cy - rad:.3f} {cx + rad:.3f} {cy - k:.3f} {cx + rad:.3f} {cy:.3f} c f Q")
            mark("vector", (cx - rad, cy - rad, cx + rad, cy + rad), ["fill"])
        elif kind == "clipped":
            x0, y0, x1, y1 = x + 8, y + 8, x + 120, y + 50
            if rng.random() < 0.3:
                # the clip is elsewhere in the cell: nothing shows
                ops.append(f"q {x + 150:g} {y + 8:g} 20 20 re W n 0 0 0 rg {x0:g} {y0:g} 100 20 re f Q")
                mark("vector", (x0, y0, x0 + 100, y0 + 20), ["fill", "hidden"], box_checked=False)
            else:
                ops.append(f"q {x0 + 30:g} {y0:g} 200 100 re W n 0 0 0 rg {x0:g} {y0:g} {x1 - x0:g} {y1 - y0:g} re f Q")
                mark("vector", (x0 + 30, y0, x1, y1), ["clipped", "fill"])
        elif kind == "shading":
            x0, y0 = x + 8, y + 8
            ops.append(f"q {x0:g} {y0:g} 100 40 re W n /Sh sh Q")
            mark("vector", (x0, y0, x0 + 100, y0 + 40), ["clipped", "fill", "shading"])
        elif kind == "image":
            x0, y0, iw, ih = x + 8, y + 8, rng.uniform(20, 120), rng.uniform(10, 44)
            ops.append(f"q {iw:.2f} 0 0 {ih:.2f} {x0:g} {y0:g} cm /Im Do Q")
            mark("image", (x0, y0, x0 + round(iw, 2), y0 + round(ih, 2)))
        elif kind == "offpage":
            ops.append(f"q 0 0 0 rg {width + 50:g} {y + 8:g} 40 20 re f Q")
            mark("vector", (width + 50, y + 8, width + 90, y + 28), ["fill", "offpage"], box_checked=False)
        elif kind == "link":
            x0, y0 = x + 8, y + 8
            annots.append(b"<< /Type /Annot /Subtype /Link /Rect [%g %g %g %g] /Border [0 0 0] >>" % (x0, y0, x0 + 90, y0 + 14))
            mark("annot", (x0, y0, x0 + 90, y0 + 14), ["no_appearance"], subtype="Link")
        elif kind == "field":
            x0, y0, fw, fh = x + 8, y + 8, 120.0, 20.0
            n = 40 + len(extra)
            ap = b"q 1 w 0.5 0.5 %g %g re S Q" % (fw - 1, fh - 1)
            extra[n] = b"<< /Type /XObject /Subtype /Form /BBox [0 0 %g %g] /Length %d >>\nstream\n%s\nendstream" % (fw, fh, len(ap), ap)
            annots.append(b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (f%d) /Rect [%g %g %g %g] /AP << /N %d 0 R >> >>"
                          % (len(annots), x0, y0, x0 + fw, y0 + fh, n))
            mark("annot", (x0, y0, x0 + fw, y0 + fh), [], subtype="Widget", field="Tx")
            mark("vector", (x0, y0, x0 + fw, y0 + fh), ["annot", "stroke"])

    data = pdf_bytes(width, height, rotate, "\n".join(ops).encode(), annots, extra)
    return cid, data, {"width": width, "height": height, "rotate": rotate}, marks


def to_page(marks, data):
    """Unrotated user-space boxes to boxes on the page as displayed, by PyMuPDF's rotation matrix."""
    page = fitz.open(stream=data, filetype="pdf")[0]
    h = page.mediabox.height
    for m in marks:
        x0, y0, x1, y1 = m.pop("user_box")
        r = fitz.Rect(x0, h - y1, x1, h - y0) * page.rotation_matrix
        m["box"] = [round(v, 3) for v in (r.x0, r.y0, r.x1, r.y1)]
    return page.rect


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1])
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 200
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else SEED
    rng = random.Random(seed)
    for split in ("tune", "heldout"):
        (out / split).mkdir(parents=True, exist_ok=True)
    items = []
    for i in range(n):
        cid, data, page, marks = build_case(rng, i)
        split = split_of(cid)
        rect = to_page(marks, data)
        f = f"{split}/{cid}.pdf"
        (out / f).write_bytes(data)
        items.append({"id": cid, "file": f, "split": split, **page, "shown": [rect.width, rect.height], "marks": marks,
                      "sha256": hashlib.sha256(data).hexdigest()})
    manifest = {"seed": seed, "cases": n, "items": items}
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")
    counts = {}
    for it in items:
        for m in it["marks"]:
            counts[m["what"]] = counts.get(m["what"], 0) + 1
    print(f"{n} cases ({sum(it['split'] == 'tune' for it in items)} tune), marks: {counts}")


if __name__ == "__main__":
    main()
