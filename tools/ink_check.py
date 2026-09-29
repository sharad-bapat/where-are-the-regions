"""Ink test: does the map cover everything the page draws? (plans/page-map.md, section 6)

Renders each page with PyMuPDF, independently of our parser, and compares the ink with the map
regions-cli gives for it:

  ink        a pixel darker than INK_LEVEL on any channel (the page background is white)
  map        every visible region's box, grown by MARGIN_PT for anti-aliasing: the images, the
             visible words and the painted paths (invisible and offpage marks are flagged, not ink)
  coverage   share of ink pixels inside the map, per page
  missed     ink outside the map, attributed to what PyMuPDF says is there, first match wins:
             text (its characters), image (its image boxes), vector (its drawings), annot
             (annotation and widget boxes), other
  phantoms   regions whose own box holds no ink at all (a blank image, white text); reported,
             not failed, since some are real

usage:
  python tools/ink_check.py constructed [split=tune] [--worst=N] [--out=file.jsonl]
  python tools/ink_check.py real <labels.jsonl> <folder of PDFs> [--worst=N] [--out=file.jsonl]

Held-out data (constructed heldout, govdocs1 004) is refused until the rules are frozen.
"""
import json
import statistics
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

import fitz
import numpy as np

ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT / "regions" / "target" / "release" / "regions-cli.exe"
DPI = 150
INK_LEVEL = 250
MARGIN_PT = 1.0
KINDS = ["text", "image", "vector", "annot", "other"]


def run(files):
    """One JSON line per file, from the binary; Windows paths so the .exe reads them."""
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
        f.write("\n".join(str(p.resolve()).replace("\\", "/") for p in files))
        lst = f.name
    out = subprocess.run([str(CLI), "--list", lst], capture_output=True, text=True, encoding="utf-8", check=True).stdout
    Path(lst).unlink()
    return {Path(d["file"]).name: d for d in map(json.loads, out.splitlines())}


def fill(mask, box, scale, grow=0.0):
    """Set the pixels of box (points, top-left origin) in mask."""
    h, w = mask.shape
    x0 = max(int((box[0] - grow) * scale), 0)
    y0 = max(int((box[1] - grow) * scale), 0)
    x1 = min(int(np.ceil((box[2] + grow) * scale)), w)
    y1 = min(int(np.ceil((box[3] + grow) * scale)), h)
    if x1 > x0 and y1 > y0:
        mask[y0:y1, x0:x1] = True


def ink_of(page):
    pix = page.get_pixmap(dpi=DPI, alpha=False)
    a = np.frombuffer(pix.samples, dtype=np.uint8).reshape(pix.height, pix.width, pix.n)
    return (a.min(axis=2) < INK_LEVEL), pix.width / page.rect.width


def pymupdf_masks(page, shape, scale):
    """What PyMuPDF says is on the page, one mask per kind, for attributing missed ink.

    PyMuPDF gives these boxes on the unrotated page but renders the page turned by /Rotate, as the
    map's boxes are, so each is mapped through the page's rotation matrix first."""
    m = {k: np.zeros(shape, bool) for k in KINDS[:-1]}
    rot = page.rotation_matrix

    def put(kind, box, grow):
        r = fitz.Rect(box) * rot
        fill(m[kind], (r.x0, r.y0, r.x1, r.y1), scale, grow)

    for b in page.get_text("rawdict")["blocks"]:
        for line in b.get("lines", []):
            for span in line["spans"]:
                for c in span["chars"]:
                    # a space has a box too, and background ink showing through it isn't text
                    if not c["c"].isspace():
                        put("text", c["bbox"], MARGIN_PT)
    for info in page.get_image_info():
        put("image", info["bbox"], MARGIN_PT)
    for d in page.get_drawings():
        put("vector", d["rect"], MARGIN_PT + (d.get("width") or 0) / 2)
    for a in page.annots() or []:
        put("annot", a.rect, MARGIN_PT)
    for wd in page.widgets() or []:
        put("annot", wd.rect, MARGIN_PT)
    return m


def check(page, pmap):
    """One page: coverage, missed ink by kind, phantoms."""
    ink, scale = ink_of(page)
    covered = np.zeros(ink.shape, bool)
    phantoms = Counter()
    regions = [("image", r, "offpage" in [w for w, _ in r["reasons"]]) for r in pmap["regions"]]
    regions += [("word", w, w.get("invisible") or w.get("offpage") or w.get("hidden")) for w in pmap["words"]]
    # white paths paint nothing on a white page: in the map, but never phantoms. A dot (a point
    # stroked with round caps) is counted apart: MuPDF 1.24 doesn't paint "m h" dots (findings.md).
    # An empty path (a point with no dot) paints nothing, so it's flagged.
    kind = lambda p: "white_path" if p.get("white") else "dot_path" if p.get("dot") else "path"
    regions += [(kind(p), p, p.get("offpage") or p.get("hidden") or p.get("empty")) for p in pmap.get("paths", [])]
    # annotations that draw; a link or a field with no appearance paints nothing, so it's flagged
    regions += [("annot", a, not a.get("appearance") or a.get("hidden") or a.get("offpage")) for a in pmap.get("annots", [])]
    for what, r, flagged in regions:
        if flagged:
            continue
        box = (r["x0"], r["y0"], r["x1"], r["y1"])
        fill(covered, box, scale, MARGIN_PT)
        own = np.zeros(ink.shape, bool)
        fill(own, box, scale)
        if own.any() and not (ink & own).any():
            phantoms[what] += 1
    total = int(ink.sum())
    missed = ink & ~covered
    by_kind = {}
    if missed.any():
        left = missed.copy()
        for k, m in pymupdf_masks(page, ink.shape, scale).items():
            by_kind[k] = int((left & m).sum())
            left &= ~m
        by_kind["other"] = int(left.sum())
    cov = 1.0 if total == 0 else 1 - missed.sum() / total
    return {"ink": total, "coverage": cov, "missed": by_kind, "phantoms": dict(phantoms)}


def pages_constructed(split):
    if split != "tune":
        sys.exit("only the tune split is checked until the rules are frozen")
    man = json.loads((ROOT / "data" / "constructed" / "manifest.json").read_text(encoding="utf-8"))
    return [(ROOT / "data" / "constructed" / c["file"], 0, c["kind"]) for c in man["items"] if c["split"] == split]


def pages_real(labels, folder):
    if "004" in Path(labels).name or Path(folder).name == "004":
        sys.exit("govdocs1 004 is held out until the rules are frozen")
    rows = [json.loads(l) for l in Path(labels).read_text(encoding="utf-8").splitlines() if l.strip()]
    return [(Path(folder) / r["file"], r["page"], "real") for r in rows]


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opt = dict(a[2:].split("=", 1) for a in sys.argv[1:] if a.startswith("--") and "=" in a)
    worst = int(opt.get("worst", 10))
    if not args or args[0] not in ("constructed", "real"):
        sys.exit(__doc__)
    pages = pages_constructed(args[1] if len(args) > 1 else "tune") if args[0] == "constructed" else pages_real(args[1], args[2])
    maps = run(sorted({p for p, _, _ in pages}))

    rows = []
    for path, pno, group in pages:
        d = maps[path.name]
        if "error" in d or d.get("status") not in (None, "ok"):
            rows.append({"file": path.name, "page": pno, "group": group, "error": d.get("error") or d.get("status")})
            continue
        with fitz.open(path) as doc:
            r = check(doc[pno], d["pages"][pno])
        rows.append({"file": path.name, "page": pno, "group": group, **r})

    ok = [r for r in rows if "error" not in r]
    print(f"{len(rows)} pages, {len(rows) - len(ok)} errors, {DPI} dpi, ink < {INK_LEVEL}, margin {MARGIN_PT} pt")
    groups = defaultdict(list)
    for r in ok:
        groups[r["group"]].append(r)
    print(f"{'group':<11} {'pages':>5} {'100%':>6} {'>=99.9%':>8} {'>=99.5%':>8}   median  worst")
    for g, rs in sorted(groups.items()) + ([("all", ok)] if len(groups) > 1 else []):
        c = [r["coverage"] for r in rs]
        print(f"{g:<11} {len(rs):>5} {sum(x == 1 for x in c):>6} {sum(x >= 0.999 for x in c):>8} {sum(x >= 0.995 for x in c):>8}"
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
    print(f"phantoms (box with no ink): images {ph['image']}, words {ph['word']}, paths {ph['path']}"
          f" (white paths with no ink, expected: {ph['white_path']}; dots MuPDF doesn't paint: {ph['dot_path']})")
    print(f"worst {worst}:")
    for r in sorted(ok, key=lambda r: r["coverage"])[:worst]:
        print(f"  {r['file']} p{r['page']} {r['group']} {100 * r['coverage']:.2f}% missed {r['missed']} phantoms {r['phantoms']}")
    for r in rows:
        if "error" in r:
            print(f"  error {r['file']}: {r['error']}")
    if "out" in opt:
        Path(opt["out"]).write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
