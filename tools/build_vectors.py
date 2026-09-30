"""Build the vector set: one-page PDFs, each drawing one kind of vector mark whose kind is known by
construction (chunk 9, D90), for scoring regions' vector kinds.

  rule      one stroked line, horizontal or vertical
  border    a stroked frame round an area, sometimes a double one
  table     a ruled grid of cells (lines or per-cell rectangles) with text in the cells
  fill      a filled area: a shaded box, a rounded box, a band, sometimes with text on it
  chart     a bar chart, a line plot or a pie (reportlab.graphics), or a box-and-arrow diagram
  outlined  words drawn as glyph outlines (fontTools, the Vera fonts reportlab ships), one filled
            path per letter, as converters that outline text do

Each case records its kind and the box of the area it draws in (points from the top left); the
scorer judges the largest vector cluster inside that area, and for outlined text the paths in it.
Text in tables, fills and charts is real text, not vectors. The split (tune or heldout) is by a hash
of the case id. Deterministic: reportlab's invariant mode, a fixed seed, so a rebuild is
byte-identical.

usage: python tools/build_vectors.py <outdir> [cases=240] [seed]
"""
import hashlib
import json
import os
import random
import sys
from pathlib import Path

import reportlab
from fontTools.pens.basePen import BasePen
from fontTools.ttLib import TTFont
from reportlab.graphics import renderPDF
from reportlab.graphics.charts.barcharts import VerticalBarChart
from reportlab.graphics.charts.lineplots import LinePlot
from reportlab.graphics.charts.piecharts import Pie
from reportlab.graphics.shapes import Drawing
from reportlab.lib import colors
from reportlab.pdfgen import canvas

SEED = 20260930
KINDS = ["rule", "border", "table", "fill", "chart", "outlined"]
W, H = 612.0, 792.0
FONT_DIR = Path(reportlab.__file__).parent / "fonts"
TTFS = ["Vera.ttf", "VeraBd.ttf", "VeraIt.ttf"]
WORDS = "notice party agreement term confidential information shall means within days written consent".split()


def split_of(cid):
    return "heldout" if hashlib.sha256(cid.encode()).digest()[0] % 2 else "tune"


class PathPen(BasePen):
    """Glyph outlines into a reportlab path, scaled and moved (quadratic curves as cubic)."""

    def __init__(self, glyphset, path, scale, dx, dy):
        super().__init__(glyphset)
        self.p, self.s, self.dx, self.dy = path, scale, dx, dy

    def _pt(self, pt):
        return self.dx + pt[0] * self.s, self.dy + pt[1] * self.s

    def _moveTo(self, pt):
        self.p.moveTo(*self._pt(pt))

    def _lineTo(self, pt):
        self.p.lineTo(*self._pt(pt))

    def _curveToOne(self, a, b, c):
        self.p.curveTo(*self._pt(a), *self._pt(b), *self._pt(c))

    def _closePath(self):
        self.p.close()


def area(rng, w_range, h_range):
    w, h = rng.uniform(*w_range), rng.uniform(*h_range)
    x, y = rng.uniform(40, W - 40 - w), rng.uniform(40, H - 40 - h)
    return x, y, w, h  # PDF coordinates, bottom left


def draw(c, kind, rng, fonts):
    """Draw one case; returns its area (x, y, w, h) in PDF coordinates and a note."""
    if kind == "rule":
        lw, length = rng.choice([0.5, 1, 1.5, 2]), rng.uniform(100, 450)
        c.setLineWidth(lw)
        if rng.random() < 0.7:
            x, y = rng.uniform(40, W - 40 - length), rng.uniform(60, H - 60)
            c.line(x, y, x + length, y)
            return (x - 2, y - 2, length + 4, 4), "horizontal"
        x, y = rng.uniform(60, W - 60), rng.uniform(40, H - 40 - length)
        c.line(x, y, x, y + length)
        return (x - 2, y - 2, 4, length + 4), "vertical"
    if kind == "border":
        x, y, w, h = area(rng, (150, 500), (100, 600))
        c.setLineWidth(rng.choice([0.5, 1, 2, 3]))
        c.rect(x, y, w, h, stroke=1, fill=0)
        note = "single"
        if rng.random() < 0.3:
            c.rect(x + 5, y + 5, w - 10, h - 10, stroke=1, fill=0)
            note = "double"
        c.setFont("Helvetica", 10)
        c.drawString(x + 15, y + h / 2, " ".join(rng.sample(WORDS, 4)))
        return (x - 3, y - 3, w + 6, h + 6), note
    if kind == "table":
        rows, cols = rng.randint(3, 12), rng.randint(2, 6)
        ch, cw = rng.uniform(14, 24), rng.uniform(40, 110)
        x, y = rng.uniform(40, max(41, W - 40 - cols * cw)), rng.uniform(40, max(41, H - 40 - rows * ch))
        c.setLineWidth(rng.choice([0.5, 0.75, 1]))
        style = rng.choice(["lines", "cells"])
        if style == "lines":
            for r in range(rows + 1):
                c.line(x, y + r * ch, x + cols * cw, y + r * ch)
            for k in range(cols + 1):
                c.line(x + k * cw, y, x + k * cw, y + rows * ch)
        else:
            for r in range(rows):
                for k in range(cols):
                    c.rect(x + k * cw, y + r * ch, cw, ch, stroke=1, fill=0)
        c.setFont("Helvetica", 8)
        for r in range(rows):
            for k in range(cols):
                c.drawString(x + k * cw + 3, y + r * ch + 4, rng.choice(WORDS)[: max(3, int(cw / 7))])
        return (x - 2, y - 2, cols * cw + 4, rows * ch + 4), f"{style} {rows}x{cols}"
    if kind == "fill":
        x, y, w, h = area(rng, (50, 500), (20, 400))
        c.setFillColor(rng.choice([colors.lightgrey, colors.lightblue, colors.beige, colors.darkblue, colors.pink]))
        style = rng.choice(["box", "rounded", "band"])
        if style == "rounded":
            c.roundRect(x, y, w, h, min(w, h) / 6, stroke=0, fill=1)
        elif style == "band":
            x, w = 0, W
            c.rect(x, y, w, h, stroke=0, fill=1)
        else:
            c.rect(x, y, w, h, stroke=0, fill=1)
        if rng.random() < 0.5:
            c.setFillColor(colors.black)
            c.setFont("Helvetica", 10)
            c.drawString(x + 8, y + h / 2, " ".join(rng.sample(WORDS, 3)))
        return (x - 1, y - 1, w + 2, h + 2), style
    if kind == "chart":
        style = rng.choice(["bar", "line", "pie", "diagram"])
        dw, dh = rng.uniform(220, 400), rng.uniform(160, 300)
        x, y = rng.uniform(40, W - 40 - dw), rng.uniform(40, H - 40 - dh)
        if style == "diagram":
            n = rng.randint(3, 5)
            bw, bh = 70, 30
            c.setLineWidth(1)
            prev = None
            for k in range(n):
                bx, by = x + (k % 3) * (dw / 3), y + (k // 3) * (dh / 2)
                c.rect(bx, by, bw, bh, stroke=1, fill=0)
                c.setFont("Helvetica", 8)
                c.drawString(bx + 5, by + 12, rng.choice(WORDS))
                if prev:
                    # an arrow from the previous box's right edge to this box, with a filled head
                    x0, y0, x1, y1 = prev[0] + bw, prev[1] + bh / 2, bx, by + bh / 2
                    c.line(x0, y0, x1, y1)
                    p = c.beginPath()
                    p.moveTo(x1, y1); p.lineTo(x1 - 6, y1 + 3); p.lineTo(x1 - 6, y1 - 3); p.close()
                    c.drawPath(p, stroke=0, fill=1)
                prev = (bx, by)
            return (x - 2, y - 2, dw + bw, dh + bh), "diagram"
        d = Drawing(dw, dh)
        if style == "bar":
            ch_ = VerticalBarChart()
            ch_.x, ch_.y, ch_.width, ch_.height = 30, 25, dw - 45, dh - 40
            ch_.data = [[rng.randint(1, 20) for _ in range(rng.randint(3, 8))]]
            ch_.categoryAxis.categoryNames = [w[:4] for w in rng.sample(WORDS, len(ch_.data[0]))]
        elif style == "line":
            ch_ = LinePlot()
            ch_.x, ch_.y, ch_.width, ch_.height = 30, 25, dw - 45, dh - 40
            ch_.data = [[(i, rng.uniform(0, 10)) for i in range(rng.randint(5, 15))]]
        else:
            ch_ = Pie()
            ch_.x, ch_.y, ch_.width, ch_.height = 20, 20, min(dw, dh) - 40, min(dw, dh) - 40
            ch_.data = [rng.randint(1, 10) for _ in range(rng.randint(3, 7))]
        d.add(ch_)
        renderPDF.draw(d, c, x, y)
        return (x, y, dw, dh), style
    # outlined text: each letter one filled path
    font = fonts[rng.randrange(len(fonts))]
    gs, cmap, upm = font.getGlyphSet(), font.getBestCmap(), font["head"].unitsPerEm
    size = rng.uniform(10, 36)
    s = size / upm
    words = rng.sample(WORDS, rng.randint(1, 4))
    x0, y0 = rng.uniform(40, 200), rng.uniform(60, H - 80)
    x = x0
    c.setFillColor(colors.black)
    for word in words:
        for ch in word:
            g = cmap.get(ord(ch))
            if g is None:
                continue
            p = c.beginPath()
            gs[g].draw(PathPen(gs, p, s, x, y0))
            c.drawPath(p, stroke=0, fill=1)
            x += gs[g].width * s
        x += gs[cmap[32]].width * s
    return (x0 - 2, y0 - 0.25 * size, x - x0 + 4, 1.25 * size), f"{len(words)} words at {size:.1f} pt"


def main():
    out = Path(sys.argv[1])
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 240
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else SEED
    rng = random.Random(seed)
    fonts = [TTFont(FONT_DIR / f) for f in TTFS]
    for split in ("tune", "heldout"):
        (out / split).mkdir(parents=True, exist_ok=True)
    items = []
    for i in range(n):
        kind = KINDS[i % len(KINDS)]
        cid = f"v{i:04d}"
        split = split_of(cid)
        path = out / split / f"{cid}.pdf"
        c = canvas.Canvas(str(path), pagesize=(W, H), invariant=1)
        c.setTitle(cid)
        (ax, ay, aw, ah), note = draw(c, kind, rng, fonts)
        c.showPage()
        c.save()
        data = path.read_bytes()
        # the area in points from the top left, as the map's boxes are
        box = [round(ax, 2), round(H - (ay + ah), 2), round(ax + aw, 2), round(H - ay, 2)]
        items.append({"id": cid, "kind": kind, "note": note, "box": box, "split": split,
                      "file": f"{split}/{cid}.pdf", "sha256": hashlib.sha256(data).hexdigest()})
    (out / "manifest.json").write_text(json.dumps({"seed": seed, "cases": n, "items": items}, indent=1), encoding="utf-8")
    counts = {s: sum(1 for it in items if it["split"] == s) for s in ("tune", "heldout")}
    print(f"{n} cases in {out}: {counts}")


if __name__ == "__main__":
    main()
