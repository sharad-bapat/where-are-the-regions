"""Make the demo's sample PDFs. All content is made up; the same files come out every run.

  report.pdf  a one-page report: heading and rule, body text, a ruled table, a bar chart drawn as
              vectors, and a framed photo
  scan.pdf    a scanned memo: one greyscale image of typed text, with an invisible OCR text layer
  hidden.pdf  marks a reader can't see: white text, text cut away by a clip, an image half off the
              page and one wholly off it, a white box, and a link with no appearance
  slide.pdf   a landscape slide: a coloured title band, a title drawn as letter outlines, a pie chart,
              a logo made of shapes, and a frame round the page

usage: python tools/samples.py [out dir, default demo/samples]
"""
import io
import random
import sys
from pathlib import Path

import reportlab
from fontTools.pens.basePen import BasePen
from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFilter, ImageFont
from reportlab.graphics import renderPDF
from reportlab.graphics.charts.barcharts import VerticalBarChart
from reportlab.graphics.charts.piecharts import Pie
from reportlab.graphics.shapes import Drawing
from reportlab.lib import colors
from reportlab.lib.utils import ImageReader
from reportlab.pdfgen import canvas

SEED = 20261002
FONT_DIR = Path(reportlab.__file__).parent / "fonts"
W, H = 612.0, 792.0


class PathPen(BasePen):
    """Glyph outlines into a reportlab path, scaled and moved (as in tools/build_vectors.py)."""

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


def photo(w, h, rng):
    """A made-up photo: a sky-to-ground gradient, soft blobs and grain."""
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        t = y / h
        for x in range(w):
            px[x, y] = (int(90 + 100 * t), int(140 + 60 * (1 - t)), int(200 - 120 * t))
    d = ImageDraw.Draw(img)
    for _ in range(12):
        cx, cy, r = rng.randrange(w), rng.randrange(h // 2, h), rng.randrange(10, 40)
        d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=(rng.randrange(40, 120), rng.randrange(80, 160), rng.randrange(30, 90)))
    img = img.filter(ImageFilter.GaussianBlur(3))
    px = img.load()
    for y in range(h):
        for x in range(w):
            n = rng.randrange(-12, 13)
            r, g, b = px[x, y]
            px[x, y] = (max(0, min(255, r + n)), max(0, min(255, g + n)), max(0, min(255, b + n)))
    return img


def report(path, rng):
    c = canvas.Canvas(str(path), pagesize=(W, H), invariant=1)
    c.setFont("Helvetica-Bold", 20)
    c.drawString(72, H - 90, "Allotment society: spring report")
    c.setLineWidth(1)
    c.line(72, H - 100, W - 72, H - 100)
    c.setFont("Times-Roman", 11)
    body = ["The society had 42 plots in use this spring, three more than last year. The new",
            "water butts on the north side filled in March, and the shared shed now has a",
            "lock. The table below gives the plots by size, and the chart the produce sold."]
    for i, line in enumerate(body):
        c.drawString(72, H - 130 - 15 * i, line)
    # a ruled table: three columns, five rows
    x0, y0, cw, rh = 72, H - 300, 140, 22
    rows = [("Plot size", "Plots", "Waiting"), ("Quarter", "18", "6"), ("Half", "16", "4"), ("Full", "8", "2"), ("All", "42", "12")]
    for r in range(len(rows) + 1):
        c.line(x0, y0 + 110 - r * rh, x0 + 3 * cw, y0 + 110 - r * rh)
    for k in range(4):
        c.line(x0 + k * cw, y0 + 110, x0 + k * cw, y0 + 110 - len(rows) * rh)
    for r, row in enumerate(rows):
        c.setFont("Helvetica-Bold" if r == 0 else "Helvetica", 10)
        for k, cell in enumerate(row):
            c.drawString(x0 + k * cw + 6, y0 + 110 - r * rh - 15, cell)
    # a bar chart, drawn as vectors
    d = Drawing(260, 170)
    ch = VerticalBarChart()
    ch.x, ch.y, ch.width, ch.height = 30, 20, 210, 130
    ch.data = [[12, 19, 7, 15]]
    ch.categoryAxis.categoryNames = ["Beans", "Kale", "Leeks", "Peas"]
    ch.valueAxis.valueMin = 0
    ch.bars[0].fillColor = colors.HexColor("#4a7f5a")
    d.add(ch)
    renderPDF.draw(d, c, 60, 150)
    # a framed photo
    img = photo(220, 160, rng)
    c.drawImage(ImageReader(img), 340, 170, width=200, height=145)
    c.setLineWidth(2)
    c.rect(336, 166, 208, 153)
    c.setFont("Times-Italic", 9)
    c.drawString(340, 152, "The north beds in April")
    c.showPage()
    c.save()


def scan(path, rng):
    """A memo typed onto a page and scanned: an image at 150 dpi, then OCR text in render mode 3."""
    dpi = 150
    pw, ph = int(W / 72 * dpi), int(H / 72 * dpi)
    img = Image.new("L", (pw, ph), 250)
    d = ImageDraw.Draw(img)
    big = ImageFont.truetype(str(FONT_DIR / "VeraBd.ttf"), 36)
    small = ImageFont.truetype(str(FONT_DIR / "Vera.ttf"), 25)
    lines = [(big, "Memo"), (small, ""), (small, "To: the plot holders"), (small, "From: the committee"), (small, ""),
             (small, "The water will be turned off on Saturday morning while the"), (small, "pipe by the gate is mended."),
             (small, "Please fill your cans on Friday evening."), (small, ""), (small, "Thank you.")]
    y = 160
    placed = []
    for f, text in lines:
        if text:
            d.text((150, y), text, fill=30, font=f)
            placed.append((text, 150, y, f.size))
        y += int(f.size * 1.6)
    img = img.filter(ImageFilter.GaussianBlur(0.6))
    px = img.load()
    for _ in range(9000):
        x, yy = rng.randrange(pw), rng.randrange(ph)
        px[x, yy] = max(0, px[x, yy] - rng.randrange(20, 90))
    c = canvas.Canvas(str(path), pagesize=(W, H), invariant=1)
    c.drawImage(ImageReader(img), 0, 0, width=W, height=H)
    t = c.beginText()
    t.setTextRenderMode(3)
    for text, x, yy, size in placed:
        pt = size * 72 / dpi
        t.setFont("Helvetica", pt * 0.9)
        t.setTextOrigin(x * 72 / dpi, H - (yy + size * 0.8) * 72 / dpi)
        t.textLine(text)
    c.drawText(t)
    c.showPage()
    c.save()


def hidden(path, rng):
    c = canvas.Canvas(str(path), pagesize=(W, H), invariant=1)
    c.setFont("Helvetica-Bold", 18)
    c.drawString(72, H - 90, "Marks you can't see")
    c.setFont("Helvetica", 11)
    c.drawString(72, H - 130, "1. A line of white text sits below this one.")
    c.setFillColor(colors.white)
    c.drawString(72, H - 150, "This sentence is printed in white on a white page.")
    c.setFillColor(colors.black)
    c.drawString(72, H - 190, "2. Text drawn inside a clip that hides all of it follows.")
    c.saveState()
    p = c.beginPath()
    p.rect(400, 100, 40, 20)
    c.clipPath(p, stroke=0, fill=0)
    c.drawString(72, H - 210, "This sentence is drawn, but its clip is elsewhere.")
    c.restoreState()
    c.drawString(72, H - 250, "3. A photo half off the right edge of the page, and one wholly off it.")
    img = ImageReader(photo(200, 140, rng))
    c.drawImage(img, W - 110, H - 420, width=200, height=140)
    c.drawImage(img, W + 50, H - 420, width=200, height=140)
    c.drawString(72, H - 460, "4. A white box, and a link with nothing drawn for it.")
    c.setFillColor(colors.white)
    c.rect(72, H - 540, 160, 50, stroke=0, fill=1)
    c.setFillColor(colors.black)
    c.linkURL("https://example.com/", (300, H - 540, 460, H - 490), relative=0, thickness=0)
    c.showPage()
    c.save()


def slide(path, rng):
    sw, sh = 720.0, 540.0
    c = canvas.Canvas(str(path), pagesize=(sw, sh), invariant=1)
    c.setLineWidth(1.5)
    c.rect(12, 12, sw - 24, sh - 24)
    c.setFillColor(colors.HexColor("#1f4e79"))
    c.rect(12, sh - 100, sw - 24, 88, stroke=0, fill=1)
    # the title as letter outlines, in white on the band
    font = TTFont(str(FONT_DIR / "VeraBd.ttf"))
    gs, cmap, upm = font.getGlyphSet(), font.getBestCmap(), font["head"].unitsPerEm
    size, x = 34, 40
    s = size / upm
    c.setFillColor(colors.white)
    for ch in "Harvest review":
        g = cmap.get(ord(ch))
        if g is None:
            continue
        p = c.beginPath()
        gs[g].draw(PathPen(gs, p, s, x, sh - 70))
        c.drawPath(p, stroke=0, fill=1)
        x += gs[g].width * s
    c.setFillColor(colors.black)
    c.setFont("Helvetica", 16)
    for i, line in enumerate(["Produce sold, by crop", "Peas and beans did best", "Next year: more leeks"]):
        c.drawString(60, sh - 160 - 30 * i, line)
    d = Drawing(260, 260)
    pie = Pie()
    pie.x, pie.y, pie.width, pie.height = 30, 30, 200, 200
    pie.data = [12, 19, 7, 15]
    pie.labels = ["Beans", "Kale", "Leeks", "Peas"]
    for i, col in enumerate(["#4a7f5a", "#9bbf6a", "#d9c25b", "#c8784a"]):
        pie.slices[i].fillColor = colors.HexColor(col)
    d.add(pie)
    renderPDF.draw(d, c, 400, 120)
    # a logo made of shapes: a leaf in a circle
    c.setFillColor(colors.HexColor("#4a7f5a"))
    c.circle(90, 80, 34, stroke=0, fill=1)
    c.setFillColor(colors.white)
    p = c.beginPath()
    p.moveTo(70, 66)
    p.curveTo(72, 100, 100, 104, 112, 98)
    p.curveTo(108, 72, 86, 62, 70, 66)
    c.drawPath(p, stroke=0, fill=1)
    c.showPage()
    c.save()


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "demo/samples")
    out.mkdir(parents=True, exist_ok=True)
    for name, make in (("report", report), ("scan", scan), ("hidden", hidden), ("slide", slide)):
        make(out / f"{name}.pdf", random.Random(f"{SEED}-{name}"))
        print(out / f"{name}.pdf")


if __name__ == "__main__":
    main()
