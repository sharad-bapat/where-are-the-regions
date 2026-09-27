"""Build the constructed test set: one-page PDFs whose regions are known by construction.

Each case starts from a real digital NDA page (ContractNLI, CC BY 4.0) and gets at most one pasted
image at a known box. What is pasted, and how, decides the expected answer:

  text        a crop of another contract page, rasterised (needs OCR in that box)
  text_ocr    the same, run through Tesseract so an invisible text layer sits on it (no OCR needed)
  photo       smooth random blobs, no text (no OCR needed; option A is expected to flag it)
  logo        flat geometric shapes, no letters (as photo)
  blank       a blank scanned sheet: paper grey plus noise (as photo)
  rule        a thin decorative bar a few pixels high (never a region)
  background  a light texture under the whole page's digital text (never a region)
  control     nothing pasted (no regions)
  full_scan   the whole page rasterised, image only (one full-page region)

Placement varies: pasted into the page's largest empty band when one is big enough, otherwise the page
is shrunk to make room; as one image, split into strips, wrapped in a form XObject, or as an inline image.
Rasters vary in DPI, JPEG or lossless, greyscale or bitonal, noise and a small rotation.

Boxes are in PDF points with the origin at the top left of the page (PyMuPDF's convention).
The split (tune or held-out) is by a hash of the source file, so no source page is in both.

usage: python tools/build_set.py <contract-nli raw dir> <labels-real.json> <outdir> [cases=600] [seed]
"""
import hashlib
import io
import json
import random
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

import fitz
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

SEED = 20260927
KINDS = ["text"] * 6 + ["text_ocr"] * 2 + ["photo", "logo", "blank", "rule", "background", "control", "full_scan"]
WRAPS = ["image", "image", "strips", "form", "inline"]
DPIS = [150, 200, 300]
MIN_WORDS = 25  # a text crop must hold at least this many real words from its source


def split_of(source):
    return "heldout" if hashlib.sha256(source.encode()).digest()[0] % 2 else "tune"


# ---- rasters ---------------------------------------------------------------------------------

def text_crop(rng, pages, w, h):
    """Crop a w x h point box of text from a random source page; return (PIL image at 72 dpi units, words, page ref)."""
    for _ in range(40):
        doc, pno = rng.choice(pages)
        page = doc[pno]
        r = page.rect
        if r.width < w or r.height < h:
            continue
        x, y = rng.uniform(0, r.width - w), rng.uniform(0, r.height - h)
        clip = fitz.Rect(x, y, x + w, y + h)
        words = page.get_text("words", clip=clip)
        if len(words) >= MIN_WORDS:
            return page, clip, len(words)
    return None


def render(page, clip, dpi):
    pix = page.get_pixmap(dpi=dpi, clip=clip, colorspace=fitz.csGRAY)
    return Image.frombytes("L", (pix.width, pix.height), pix.samples)


def synth_image(rng, kind, px_w, px_h):
    nr = np.random.default_rng(rng.randrange(1 << 30))
    if kind == "photo":
        small = nr.random((max(2, px_h // 40), max(2, px_w // 40), 3))
        img = Image.fromarray((small * 255).astype(np.uint8), "RGB").resize((px_w, px_h), Image.BICUBIC)
        return img.filter(ImageFilter.GaussianBlur(3))
    if kind == "logo":
        img = Image.new("RGB", (px_w, px_h), "white")
        d = ImageDraw.Draw(img)
        for _ in range(rng.randint(2, 5)):
            c = tuple(rng.randrange(256) for _ in range(3))
            x0, y0 = rng.randrange(px_w), rng.randrange(px_h)
            box = [x0, y0, min(px_w, x0 + rng.randint(px_w // 6, px_w // 2)), min(px_h, y0 + rng.randint(px_h // 6, px_h // 2))]
            (d.ellipse if rng.random() < 0.5 else d.rectangle)(box, fill=c)
        return img
    if kind in ("blank", "background"):
        base = 250 if kind == "background" else 236
        a = np.clip(base + nr.normal(0, 4, (px_h, px_w)), 0, 255).astype(np.uint8)
        return Image.fromarray(a, "L")
    if kind == "rule":
        c = tuple(rng.randrange(160) for _ in range(3))
        return Image.new("RGB", (px_w, px_h), c)
    raise ValueError(kind)


def scanner(rng, img, bitonal):
    """Scanner-style damage: a small rotation, noise, and bitonal or greyscale."""
    angle = rng.uniform(-1.5, 1.5)
    img = img.convert("L").rotate(angle, resample=Image.BICUBIC, fillcolor=255)
    a = np.asarray(img, dtype=np.float32) + np.random.default_rng(rng.randrange(1 << 30)).normal(0, 6, img.size[::-1])
    img = Image.fromarray(np.clip(a, 0, 255).astype(np.uint8), "L")
    return (img.point(lambda v: 255 if v > 160 else 0).convert("1") if bitonal else img), angle


def encode(img, jpeg):
    buf = io.BytesIO()
    if jpeg and img.mode != "1":
        img.save(buf, "JPEG", quality=75)
    else:
        img.save(buf, "PNG")
    return buf.getvalue()


# ---- placement --------------------------------------------------------------------------------

def free_band(page, need_h):
    """Largest empty horizontal band inside the margins, as (y0, y1), or None if it's under need_h."""
    r = page.rect
    boxes = [fitz.Rect(b[:4]) for b in page.get_text("blocks")]
    boxes += [fitz.Rect(d["rect"]) for d in page.get_drawings()]
    boxes += [fitz.Rect(i["bbox"]) for i in page.get_image_info()]
    ys = sorted((b.y0, b.y1) for b in boxes if not b.is_empty)
    top, bottom = r.height * 0.06, r.height * 0.94
    best, cur = None, top
    for y0, y1 in ys + [(bottom, bottom)]:
        if y0 - cur > (best[1] - best[0] if best else 0):
            best = (cur, min(y0, bottom))
        cur = max(cur, y1)
    return best if best and best[1] - best[0] >= need_h else None


def make_base(src_page, rng, box_h):
    """A new page with the source page's content and room for a box_h-high image. Returns (doc, page, band, how)."""
    out = fitz.open()
    out.insert_pdf(src_page.parent, from_page=src_page.number, to_page=src_page.number)
    page = out[0]
    band = free_band(page, box_h + 12)
    if band:
        return out, page, band, "free_band"
    # Shrink the original into the top part of a fresh page and use the rest.
    r = src_page.rect
    out = fitz.open()
    page = out.new_page(width=r.width, height=r.height)
    top = rng.random() < 0.5
    content = fitz.Rect(0, box_h + 24, r.width, r.height) if top else fitz.Rect(0, 0, r.width, r.height - box_h - 24)
    page.show_pdf_page(content, src_page.parent, src_page.number)
    band = (12, box_h + 18) if top else (r.height - box_h - 18, r.height - 6)
    return out, page, band, "shrunk_top" if top else "shrunk_bottom"


def inline_image(page, box, img):
    """Draw img into box as a BI ... ID ... EI inline image in the page's own content."""
    g = img.convert("L")
    data = zlib.compress(g.tobytes())
    r = page.rect
    x, y, w, h = box.x0, r.height - box.y1, box.width, box.height
    ops = (f"q {w:.3f} 0 0 {h:.3f} {x:.3f} {y:.3f} cm BI /W {g.width} /H {g.height} /CS /G /BPC 8 /F /Fl ID ".encode()
           + data + b"\nEI Q\n")
    xref = page.parent.get_new_xref()
    page.parent.update_object(xref, "<<>>")
    page.parent.update_stream(xref, ops)
    # Append the stream to /Contents (PyMuPDF has no public call for this).
    contents = page.get_contents()
    page.parent.xref_set_key(page.xref, "Contents", "[" + " ".join(f"{c} 0 R" for c in contents + [xref]) + "]")


def place(rng, doc, page, box, img, jpeg, wrap):
    if wrap == "inline":
        inline_image(page, box, img)
        return
    data = encode(img, jpeg)
    if wrap == "strips":
        n = rng.randint(3, 6)
        W, H = img.size
        for i in range(n):
            a, b = H * i // n, H * (i + 1) // n
            part = encode(img.crop((0, a, W, b)), jpeg)
            y0 = box.y0 + box.height * a / H
            y1 = box.y0 + box.height * b / H
            page.insert_image(fitz.Rect(box.x0, y0, box.x1, y1), stream=part, keep_proportion=False)
        return
    if wrap == "form":
        one = fitz.open()
        p = one.new_page(width=box.width, height=box.height)
        p.insert_image(p.rect, stream=data, keep_proportion=False)
        page.show_pdf_page(box, one, 0)
        return
    page.insert_image(box, stream=data, keep_proportion=False)


def ocr_pdf(img, dpi):
    """Tesseract over img: a one-page PDF with the image and an invisible text layer."""
    with tempfile.TemporaryDirectory() as tmp:
        png = Path(tmp) / "i.png"
        img.convert("L").save(png, dpi=(dpi, dpi))
        subprocess.run(["tesseract", str(png), str(Path(tmp) / "o"), "-l", "eng", "pdf"], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return fitz.open(Path(tmp) / "o.pdf").tobytes()


# ---- cases ------------------------------------------------------------------------------------

def build_case(rng, i, src, pno, pool_pages):
    page0 = src[pno]
    r = page0.rect
    kind = KINDS[i % len(KINDS)]
    case = {"id": f"c{i:04d}", "kind": kind, "source_page": pno, "regions": []}

    if kind == "control":
        out = fitz.open()
        out.insert_pdf(src, from_page=pno, to_page=pno)
        case["placement"] = "none"
        return out, case

    if kind == "full_scan":
        dpi = rng.choice(DPIS)
        img, _ = scanner(rng, render(page0, r, dpi), rng.random() < 0.3)
        out = fitz.open()
        p = out.new_page(width=r.width, height=r.height)
        jpeg = rng.random() < 0.5
        p.insert_image(p.rect, stream=encode(img, jpeg), keep_proportion=False)
        case.update(placement="full_page", dpi=dpi, jpeg=jpeg, words=len(page0.get_text("words")))
        case["regions"].append({"box": [0, 0, r.width, r.height], "expect": "ocr", "why": "full_scan"})
        return out, case

    if kind == "background":
        out = fitz.open()
        p = out.new_page(width=r.width, height=r.height)
        dpi = 72
        p.insert_image(p.rect, stream=encode(synth_image(rng, kind, int(r.width), int(r.height)), True), keep_proportion=False)
        p.show_pdf_page(p.rect, src, pno)
        case.update(placement="under_text", dpi=dpi, jpeg=True)
        case["regions"].append({"box": [0, 0, r.width, r.height], "expect": "none", "why": "background"})
        return out, case

    # Pick a box: from a stamp (about 2% of the page) to about half of it.
    frac = rng.choice([0.02, 0.05, 0.1, 0.2, 0.35, 0.5])
    if kind == "rule":
        w, h = r.width * rng.uniform(0.5, 0.85), rng.uniform(1.5, 4)
    else:
        aspect = rng.uniform(0.6, 3.0)
        area = frac * r.width * r.height
        w = min(r.width * 0.85, (area * aspect) ** 0.5)
        h = min(r.height * 0.6, area / w)
    out, page, band, how = make_base(page0, rng, h)
    x0 = rng.uniform(r.width * 0.06, max(r.width * 0.06, r.width * 0.94 - w))
    y0 = rng.uniform(band[0] + 6, max(band[0] + 6, band[1] - 6 - h))
    box = fitz.Rect(x0, y0, x0 + w, y0 + h)
    dpi = rng.choice(DPIS)
    jpeg = rng.random() < 0.5
    bitonal = rng.random() < 0.25
    wrap = "image" if kind == "rule" else rng.choice(WRAPS)
    case.update(placement=how, wrap=wrap, dpi=dpi, jpeg=jpeg, frac=frac)

    if kind in ("text", "text_ocr"):
        got = text_crop(rng, pool_pages, w, h)
        if not got:
            return None, None
        sp, clip, nwords = got
        img, angle = scanner(rng, render(sp, clip, dpi), bitonal)
        case.update(bitonal=bitonal, angle=round(angle, 3), words=nwords)
        if kind == "text_ocr":
            one = fitz.open("pdf", ocr_pdf(img, dpi))
            page.show_pdf_page(box, one, 0, keep_proportion=False)
            case["wrap"] = "form_ocr"
            case["regions"].append({"box": list(box), "expect": "text_layer", "why": kind})
        else:
            place(rng, out, page, box, img, jpeg, wrap)
            case["regions"].append({"box": list(box), "expect": "ocr", "why": kind})
    else:
        px_w, px_h = max(2, int(w / 72 * dpi)), max(2, int(h / 72 * dpi))
        img = synth_image(rng, kind, px_w, px_h)
        place(rng, out, page, box, img, jpeg and kind != "rule", wrap)
        case["regions"].append({"box": list(box), "expect": "none", "why": kind})
    return out, case


def main():
    raw, labels, outdir, *rest = sys.argv[1:]
    cases = int(rest[0]) if rest else 600
    seed = int(rest[1]) if len(rest) > 1 else SEED
    rows = json.loads(Path(labels).read_text(encoding="utf-8"))
    names = sorted(r["file"].replace("\\", "/").split("/raw/", 1)[1] for r in rows
                   if r.get("category") == "text" and all(p["label"] == "TEXT" and p["image_frac"] == 0 for p in r["per_page"]))
    rng = random.Random(seed)
    out = Path(outdir)
    docs = {n: fitz.open(Path(raw) / n) for n in names}
    by_split = {s: [n for n in names if split_of(n) == s] for s in ("tune", "heldout")}
    pool = {s: [(docs[n], p) for n in by_split[s] for p in range(len(docs[n]))] for s in by_split}
    manifest = {"seed": seed, "cases": cases, "kinds": KINDS, "dataset": "ContractNLI raw PDFs (CC BY 4.0)",
                "box_convention": "PDF points, origin top left", "items": []}
    i = 0
    while i < cases:
        split = "tune" if i % 2 == 0 else "heldout"
        name = rng.choice(by_split[split])
        src = docs[name]
        pno = rng.randrange(len(src))
        doc, case = build_case(rng, i, src, pno, pool[split])
        if doc is None:
            continue
        case.update(source=name, split=split)
        path = out / split / f"{case['id']}.pdf"
        path.parent.mkdir(parents=True, exist_ok=True)
        doc.set_metadata({})  # no dates, so a rebuild is byte-identical
        doc.save(path, deflate=True, garbage=3, no_new_id=True)
        case["file"] = f"{split}/{path.name}"
        case["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        manifest["items"].append(case)
        i += 1
        if i % 50 == 0:
            print(f"  {i}/{cases}", file=sys.stderr)
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1), encoding="utf-8")
    print(f"{len(manifest['items'])} cases in {out}")


if __name__ == "__main__":
    main()
