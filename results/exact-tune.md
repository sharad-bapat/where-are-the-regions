# Exact layer, tuning report (30 September 2026)

What the exact layer (every text line, image, vector cluster and annotation, with its box, drawing order and flags) gets right on the tuning data, where it falls short, and which rules and constants produced these numbers. Only tuning data is used: the marks set's tune split, the constructed tune split and govdocs1 thread 003. The held-out sets (marks heldout, constructed heldout, govdocs1 004) haven't been opened.

Source at the time of writing: commit 3548a06 plus chunk 7a-3 (ZapfDingbats glyph names, not yet committed). The rules get frozen by hash in chunk 7b, after this report, and are then measured once on the held-out sets against the targets in section 7.

## 1. Marks set

105 one-page PDFs built by tools/build_marks.py, each mark with a known box (1,106 marks). tools/score_map.py matches map regions to marks closest first, within 1 pt on every edge, and checks the flags.

| Kind | Flags | Marks | Found | Flags right | Worst found | Median error |
|---|---|---|---|---|---|---|
| annotation | none | 63 | 63 | 63 | 0.000 pt | 0.000 pt |
| annotation | no_appearance | 78 | 78 | 78 | 0.000 pt | 0.000 pt |
| image | none | 62 | 62 | 62 | 0.050 pt | 0.040 pt |
| text | none | 291 | 289 | 290 | 0.050 pt | 0.038 pt |
| text | invisible | 55 | 55 | 55 | 0.050 pt | 0.030 pt |
| vector | annot, stroke | 63 | 63 | 63 | 0.000 pt | 0.000 pt |
| vector | clipped, fill | 49 | 49 | 49 | 0.000 pt | 0.000 pt |
| vector | clipped, fill, shading | 54 | 54 | 54 | 0.000 pt | 0.000 pt |
| vector | fill | 114 | 114 | 114 | 0.055 pt | 0.030 pt |
| vector | fill, hidden | 14 | 14 | 14 | 0.000 pt | 0.000 pt |
| vector | fill, offpage | 81 | 81 | 81 | 0.000 pt | 0.000 pt |
| vector | fill, white | 28 | 28 | 28 | 0.053 pt | 0.040 pt |
| vector | stroke | 154 | 154 | 154 | 0.050 pt | 0.040 pt |
| all | | 1,106 | 1,104 (99.8%) | 1,105 | | |

No map region matches no mark. The two misses are both on m0197: two table cells whose lines sit less than 1.5 em apart merge into one text region (COLUMN_GAP, section 5). That's a known limit, left as it is rather than tuned to this one page.

## 2. Constructed set, OCR scoring

tools/score_regions.py on the 300 constructed tune cases scores regions/src/ocr.rs, which reads the map. It isn't part of the exact layer and isn't frozen with it (D76), but it runs on the same parser, so it's a check that nothing moved: text images flagged 120/120 at confidence 0.8, OCR text layers 40/40 at 0.04, full scans 20/20, controls, rules and backgrounds never flagged, box edges within 1 pt 120/120 (worst 0.05 pt). These numbers haven't changed since chunk 5b.

## 3. Ink on govdocs1 003

tools/ink_check.py renders each page with PyMuPDF 1.24.9 at 150 dpi, calls a pixel ink when any channel is below 250, and asks what share of the ink lies inside the map's visible regions, each grown by 1 pt. It is independent of the parser. Run: results/ink-003-7a3.jsonl (per page).

| Pages | At 100% | At least 99.9% | At least 99.5% | Median | Worst |
|---|---|---|---|---|---|
| 967 | 787 | 967 | 967 | 100.00% | 99.90% |

Missed ink is under 0.001% of all ink (538,606,321 pixels), on 78 pages put down to text, 3 to images and 159 to nothing PyMuPDF reports ("other"). No page lost coverage in any chunk since the per-page runs began (6d, 7a, 7a-3), checked page by page.

The misses that are left, from the worst pages:

- Glyph overhang. On 003975 and 003979 (pages 2 and 6, 28 pt bold italic) and 003867 p10 (a large Sho-Roman display letter) the ink runs up to 3 pt past the glyph's advance width. Word boxes end at the advance, as PyMuPDF's do; the outline would need the glyph program.
- Capitals above the ascent. On 003826 the BankGothic capitals rise above the font descriptor's /Ascent, so their tops fall outside the box.
- The rest are small: the "other" pixels (2,260 in all) are a few to a few hundred per page, and the text-attributed ones on 003851 (up to 150 per page) are similar. They haven't been looked at one by one; anti-aliased edges just past the 1 pt margin are the likely cause, not checked.

## 4. Phantoms on govdocs1 003

A phantom is a map region whose own box, grown by half a pixel, holds no ink. Some are real marks that don't show on a white page; each kind is sorted here.

| Kind | Count | What they are |
|---|---|---|
| Paths | 26 | 22 MuPDF lists in its own drawing list but doesn't render, while pdfium 149.0.7825.0 does (003174 p0 x21, 003413 p19; overprint suspected, not verified; results/findings.md). 3 painted over by later marks (003185 p3, 003702 p16, 003005 p1). 1 near-white 0.26 pt stroke on 003174 too faint to count as ink. |
| White paths | 217 | Painted white, expected on a white page. Includes zero-size white fills that the half-pixel test can now see. |
| Dots | 0 | Round-capped stroked points (D75). MuPDF skips "m h" dots; none land where nothing else is drawn. |
| Words | 499 | 378 are text filled white (PyMuPDF agrees), mostly US Government Printing Office "VerDate" slugs on 003077 and 003695. 52 sit inside an image's box, 8 of them drawn before an image that then covers them (003431 p4 is text under its own scan). 69 not yet looked at one by one. |
| Images | 20 | 5 are entirely white pixels. 13 are hairline or tiny images (spacers and rules under 2 pt, e.g. 003858 p2, 003190 p0) that do hold dark pixels; most likely they render too thin to reach the ink level, not checked image by image. 2 are inline images. The small masks at the page corner on 003667 are among the 20. |

The map flags white paths but not white text, so the 378 white words count as phantoms rather than expected. Flagging text drawn in a white fill colour (render mode 0 or 2) the same way would move them to the expected column; that is a possible small chunk, not done.

## 5. Rules and constants

Every constant that shapes the exact layer, where it was set, and what it was checked on.

| Constant | Value | Where | Set on |
|---|---|---|---|
| WORD_GAP | 0.15 em | lib.rs | wordbox dev (D56), carried over |
| BACKSTEP | 0.5 em | lib.rs | wordbox |
| BASELINE_SHIFT | 0.5 em | lib.rs | wordbox |
| COLUMN_GAP | 1.5 em | map.rs | marks tune (6c) |
| STRIP_EDGE, STRIP_GAP | 0.5 pt, 1.0 pt | lib.rs | constructed tune, 003 (chunk 4) |
| TOUCH_PT | 1.0 pt | vector.rs | 003 (4a) |
| LARGE_SHARE | 0.25 of the page | vector.rs | 003 (4a) |
| CELL | 36 pt (grid for neighbours; speed only) | vector.rs | |
| MAX_PAGES, MAX_FORM_DEPTH | 2,000, 8 | lib.rs | limits, not tuned |

Conventions settled along the way:

- Text boxes use the font's ascent and descent: the descriptor's, else the core-14 AFM values for the standard families; a positive /Descent is taken as negative (D73).
- Glyph widths come from /Widths, else the standard font's metrics, else for Symbol and ZapfDingbats by glyph from their built-in tables. /Differences names in ZapfDingbats fonts resolve through Adobe's zapfdingbats.txt (7a-3).
- Stroke boxes follow line caps and add half the width at joins (miter points past that are left out); a shading with a clip is flagged clipped (D73).
- A path whose points are all one point is a round-cap dot when stroked with round caps, a pixel when filled, and otherwise empty (D75, ISO 32000-1 8.5.3.2 and 8.5.3.3).
- White is read in DeviceGray, DeviceRGB, DeviceCMYK, ICCBased (by /N), CalGray and CalRGB; other colour spaces are never taken as white (D77).
- The clip in force is carried onto every mark: boxes are the visible part, hidden (clipped away) is kept apart from offpage (4b). Nothing is dropped; everything is kept and flagged (D69).

The ink test's own settings: 150 dpi, ink below 250 on any channel, 1 pt margin for coverage, half a pixel for phantoms (D77).

## 6. Not yet measured for this report

- Ink on the constructed tune split. The last run was after chunk 4a (288 of 300 pages at 100%, all at least 99.99%); every parser change since needs it rerun. It renders 300 small pages.
- Speed per page, needed before a speed target is set (D76). The last timing was per file (median 8.7 ms over the 278 files of 003, after chunk 4a), on a quiet machine.

## 7. Targets for the held-out runs (D76)

- Marks heldout: at least 99.5% of marks within 1 pt, no extra regions, every miss named.
- Constructed heldout: every page at least 99.9% ink coverage; phantoms reported by kind and each explained.
- govdocs1 004: every page at least 99.5%, at least 99% of pages at 99.9% or more, pages at 100% reported but not a target, every page under 99.9% explained.
- Speed: set after the per-page timing above.

On tune, 003 meets the 004 target (all 967 pages at 99.9% or more) and the marks set meets its target (99.8%).

## Reproduce

```
cd regions && cargo test --release && cargo build --release && cd ..
python tools/score_map.py tune
python tools/score_regions.py tune
python tools/ink_check.py constructed tune --worst=12 --out=results/ink-constructed-tune.jsonl
python tools/ink_check.py real data/real/tune-003.jsonl <govdocs1>/003 --worst=12 --out=results/ink-003.jsonl
```
