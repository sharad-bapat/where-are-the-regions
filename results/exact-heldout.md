# Exact layer, held-out results (30 September 2026)

The exact layer measured once on data it was never tuned on, with the rules frozen by hash first (results/frozen.sha256, commit 0ce9655; tools/check_frozen.py passed before every run). Targets were fixed before any held-out data was opened (D76, D79). Nothing in the frozen files changed between the runs. Two parser bugs turned up; they are reported here as found and are fixed only after this report, so the numbers below are the frozen rules' own.

The tuning results are in results/exact-tune.md.

## Summary

| Set | Target | Result | |
|---|---|---|---|
| Marks, heldout (95 pages, 1,015 marks) | at least 99.5% of marks within 1 pt, no extra regions | 1,015 of 1,015 (100%), no extras, worst 0.054 pt | met |
| Constructed, heldout (300 pages), ink | every page at least 99.9%; phantoms explained | all 300 at 99.99% or more; every phantom explained | met |
| govdocs1 004 (704 pages), ink | every page at least 99.5% | 703 of 704; 004050 p2 at 99.26% | missed by one page |
| govdocs1 004 | at least 99% of pages at 99.9% or more | 702 of 704 (99.7%) | met |
| govdocs1 004 | every page under 99.9% explained | both explained (below) | met |
| Speed | median under 5 ms a page | not rerun on held-out; 1.10 ms on tune | see section 5 |

## 1. Marks set, heldout split

tools/score_map.py heldout (results/marks-heldout.txt). Every mark found within 1 pt on every edge, every flag right, and no map region matching no mark.

| Kind | Flags | Marks | Found | Worst found |
|---|---|---|---|---|
| annotation | none / no_appearance | 76 / 67 | all | 0.000 pt |
| image | none | 56 | all | 0.050 pt |
| text | none / invisible | 240 / 65 | all | 0.050 pt |
| vector | annot, stroke | 76 | all | 0.000 pt |
| vector | clipped fill / clipped shading | 41 / 57 | all | 0.000 pt |
| vector | fill / hidden / offpage / white | 109 / 15 / 84 / 22 | all | 0.054 pt |
| vector | stroke | 107 | all | 0.050 pt |

The tune split's two misses (m0197, table cells closer than 1.5 em) have no counterpart here.

## 2. Constructed set, heldout split

OCR scoring (tools/score_regions.py heldout, results/regions-heldout.txt), the same as tune: text images flagged 120/120 with box edges within 1 pt, text layers 40/40 left alone, full scans 20/20 as one region, controls, rules and backgrounds never flagged, and photos, logos and blank images still flagged, as the structure-only rules expect (telling them apart is the kind layer's job, chunk 8).

Ink (results/ink-constructed-heldout.jsonl): 290 of 300 pages at 100%, all at least 99.99%, median 100.00%. The 10 others miss 1 to 16 pixels.

Phantoms, every one explained:

- 93 paths are zero-size filled points in the source PDFs (81 at one point on c0099, 4 each on c0201, c0441, c0495). ISO 32000-1 8.5.3.3 says such a fill paints the device pixel under the point and calls the result device-dependent; MuPDF paints none. The map keeps them unflagged, per D75.
- 3 words are one source document's footer (c0161, c0455, c0517), painted over by a white rectangle drawn after it; the render is pure white there.
- 16 white paths and 11 white words are expected.

## 3. govdocs1 004

tools/ink_check.py on the 704 labelled pages of thread 004 (results/ink-004.jsonl).

| Pages | At 100% | At least 99.9% | At least 99.5% | Median | Worst |
|---|---|---|---|---|---|
| 704 | 635 | 702 | 703 | 100.00% | 99.26% |

Missed ink is 0.001% of all ink (367,662,810 pixels), on 24 pages put down to text and 60 to other.

The two pages under 99.9%:

- 004050 p2, 99.26%, the one page under the 99.5% target. Its text is in three Type 3 fonts (FontMatrix identity, text size 0.1) whose /Widths give 0 to between 97 and 165 of their codes, while ink is drawn where those glyphs are. Boxes follow the advance width, so a zero-width glyph has a zero-width box and its ink falls outside (2,798 pixels "other"). PyMuPDF gives no character box there either. The box from the glyph procedure's d1 operands would cover it.
- 004661 p26, 99.81%. A Symbol "∆" has a zero-width box. Corrected on 30 September after the fix chunk looked at the file (this line first blamed a ToUnicode map; the font has none): the font is /Subtype /TrueType /BaseFont /Symbol with nothing embedded, no /Encoding, no /Widths and no descriptor. The parser read it with the TrueType default, WinAnsi, so the code decoded as "D", and the width lookup by glyph found no "D" in Symbol's table and gave 0. It is the standard Symbol font, substituted; PyMuPDF reads "∆".

Both are zero-width glyphs, the same class as the ZapfDingbats bug fixed during tuning (7a-3).

Phantoms on 004, sorted one file at a time:

| Kind | Count | Causes |
|---|---|---|
| Paths | 142 | 139 painted over by a later mark (79 on 004148 p1, 26 on 004987 p20, 20 on 004653 p17); 1 zero-size fill; 1 that pdfium renders and MuPDF doesn't (004983 p25); 1 not explained (004655 p2, a 6.4 x 0.9 pt fill) |
| Words | 88 | 76 inside an image's box (46 on 004989 p29); 5 painted over (004389 p16); 1 zero width (004988 p6); 6 not explained (the 3 looked at are private-use symbol characters on 004117) |
| Images | 7 | 4 tiny or thin; 3 all pixels white (004132) |
| White paths, white words | 33, 938 | expected |

## 4. Found on held-out, fixed after this report

1. A TrueType font named Symbol or ZapfDingbats with no program and no /Encoding is read with that font's built-in encoding (004661 p26; D82). Done after this report: 004661 p26 goes to 100% and its text reads "∆E".
2. A Type 3 glyph's box is the union of its advance box and the box its glyph procedure declares with d1 (004050 p2; D83). Done after this report: all four pages of 004050 go to 100%, with no zero-width words left.

Each fix is its own chunk, with a new freeze, and its effect on 004 is reported separately and marked as coming after the held-out run. The numbers above stay as the held-out result.

## 5. Speed

Not measured on the held-out files. On tune (003, 7,633 pages, results/exact-tune.md section 7) the median page takes 1.10 ms, under the 5 ms target. A held-out timing would need a quiet machine and one session; it can be added without changing anything frozen.

## Reproduce

```
python tools/check_frozen.py
python tools/score_map.py heldout --misses
python tools/score_regions.py heldout --misses
python tools/ink_check.py constructed heldout --worst=12 --out=results/ink-constructed-heldout.jsonl
python tools/ink_check.py real data/real/heldout-004.jsonl <govdocs1>/004 --worst=12 --out=results/ink-004.jsonl
```
