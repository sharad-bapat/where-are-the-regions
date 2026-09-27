"""Find real pages with both digital text and images, and label each image with Tesseract.

A page qualifies when it has at least MIN_CHARS of visible digital text and at least one image
placement of MIN_AREA of the page or more. For each such placement the reference answer comes from
the pixels, not the structure: the page is copied, its digital text is removed (redaction that keeps
images), the placement's box is rendered, and Tesseract reads it.

  text      at least TEXT_WORDS confident words: needs OCR, unless a text layer covers it
  none      no confident words
  unsure    in between; listed for a hand check and left out of the headline numbers

Boxes are in displayed page coordinates (top-left origin, after /Rotate), like regions-cli.
PyMuPDF gives image, word and text-trace boxes unrotated, so they go through page.rotation_matrix;
a pixmap clip is already in displayed coordinates.

A word counts when Tesseract is confident in it and it has at least 3 letters or 2 digits, so
axis numbers count as text (D67: any text in an image needs OCR).

It also records, per placement, how many visible digital words overlap it and whether invisible
text (render mode 3, how OCR layers hide) covers it.

usage: python tools/select_real.py <pdf dir> <out.jsonl> [max_pages=30]
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import fitz

MIN_CHARS = 40      # the same line Scan or text draws for "this page has text"
MIN_AREA = 0.01     # of the page, per placement
MIN_SIDE = 12       # points; thinner is a rule or a border
TEXT_WORDS = 5
MIN_CONF = 60
DPI = 200


def ocr_words(pix):
    with tempfile.TemporaryDirectory() as tmp:
        png = Path(tmp) / "b.png"
        pix.save(png)
        tsv = subprocess.run(["tesseract", str(png), "-", "-l", "eng", "--psm", "3", "tsv"],
                             capture_output=True, text=True, encoding="utf-8", errors="replace").stdout
    words = []
    for row in tsv.splitlines()[1:]:
        f = row.split("\t")
        if len(f) == 12 and float(f[10]) >= MIN_CONF and (sum(c.isalpha() for c in f[11]) >= 3 or sum(c.isdigit() for c in f[11]) >= 2):
            words.append(f[11])
    return words


def shown(page, rect):
    """An unrotated PyMuPDF box as displayed on the page."""
    return fitz.Rect(rect) * page.rotation_matrix


def text_spans(page):
    """(visible chars, visible word boxes, invisible span boxes) from the text trace."""
    vis, hidden = 0, []
    for s in page.get_texttrace():
        if s["type"] == 3 or s["opacity"] == 0:
            hidden.append(shown(page, s["bbox"]))
        else:
            vis += sum(1 for c in s["chars"] if chr(c[0]).strip())
    return vis, [shown(page, w[:4]) for w in page.get_text("words")], hidden


def placements(page):
    r = page.rect
    out = []
    for info in page.get_image_info(xrefs=True):
        b = shown(page, info["bbox"]) & r
        if b.is_empty or min(b.width, b.height) < MIN_SIDE or b.get_area() < MIN_AREA * r.get_area():
            continue
        out.append((b, info))
    return out


def textless(doc, pno):
    """A one-page copy with all text removed and images kept."""
    one = fitz.open()
    one.insert_pdf(doc, from_page=pno, to_page=pno)
    p = one[0]
    p.add_redact_annot(p.rect * p.derotation_matrix)  # redactions take unrotated coordinates
    p.apply_redactions(images=fitz.PDF_REDACT_IMAGE_NONE, graphics=fitz.PDF_REDACT_LINE_ART_NONE,
                       text=fitz.PDF_REDACT_TEXT_REMOVE)
    return one


def main():
    src, out_path, *rest = sys.argv[1:]
    max_pages = int(rest[0]) if rest else 30
    files = sorted(Path(src).glob("*.pdf"))
    kept = skipped = 0
    with open(out_path, "w", encoding="utf-8") as out:
        for f in files:
            try:
                doc = fitz.open(f)
                if doc.needs_pass and not doc.authenticate(""):
                    skipped += 1
                    continue
            except Exception:
                skipped += 1
                continue
            for pno in range(min(len(doc), max_pages)):
                try:
                    page = doc[pno]
                    places = placements(page)
                    if not places:
                        continue
                    vis, words, hidden = text_spans(page)
                    if vis < MIN_CHARS:
                        continue
                    bare = textless(doc, pno)[0]
                    regions = []
                    for b, info in places:
                        read = ocr_words(bare.get_pixmap(dpi=DPI, clip=b, colorspace=fitz.csGRAY))
                        n = len(read)
                        label = "text" if n >= TEXT_WORDS else "none" if n == 0 else "unsure"
                        over = sum(1 for w in words if w.intersects(b))
                        layer = sum((h & b).get_area() for h in hidden) >= 0.2 * b.get_area()
                        regions.append({"box": [round(v, 2) for v in b], "xref": info.get("xref", 0),
                                        "px": [info["width"], info["height"]], "ocr_words": n,
                                        "label": label, "digital_words_over": over, "text_layer": layer,
                                        "sample": read[:8]})
                    out.write(json.dumps({"file": f.name, "page": pno, "size": [page.rect.width, page.rect.height],
                                          "visible_chars": vis, "regions": regions}) + "\n")
                    kept += 1
                except Exception as e:
                    print(f"{f.name} p{pno}: {e}", file=sys.stderr)
    print(f"{kept} pages kept from {len(files)} files ({skipped} unreadable)")


if __name__ == "__main__":
    main()
