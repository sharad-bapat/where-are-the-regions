# Ink test, other readers as a baseline (1 October 2026)

The exact layer's coverage on govdocs1 003 means little alone, so the same ink test was run on maps built from other readers' boxes. tools/ink_baseline.py builds each map and scores it with tools/ink_check.py's own check(): PyMuPDF 1.24.9 renders at 150 dpi, a pixel is ink below 250 on any channel, every box grows by 1 pt. Tuning data only (thread 003, 967 labelled pages); nothing frozen changed.

The readers:

- pymupdf: the calls people reach for, get_text("words"), get_image_info(), get_drawings() grown by half the line width, annotations and widgets.
- bboxlog: PyMuPDF's get_bboxlog(), the box of every paint operation MuPDF makes, invisible text left out. The judge renders with MuPDF too, so this reader has home advantage.
- pdfplumber 0.11.9 (pdfminer.six 20251230): chars without real spaces, rects, lines and curves grown by half the line width, images, annotations. Its coordinates ignore the crop box (003053 p9, every box 37 pt off), so its boxes are moved onto it.

Ours is results/ink-003-post.jsonl (regions-cli after the two post-held-out fixes; identical on 003 to the tuning run 7a-4).

## Coverage

| Reader | At 100% | At least 99.9% | At least 99.5% | Median | Worst | Missed ink |
|---|---|---|---|---|---|---|
| regions-cli | 787 | 967 | 967 | 100.00% | 99.90% | 3,634 px |
| bboxlog | 966 | 966 | 966 | 100.00% | 56.42% | 710,374 px |
| pdfplumber | 722 | 943 | 955 | 100.00% | 36.55% | 781,588 px |
| pymupdf | 720 | 920 | 951 | 100.00% | 14.88% | 2,076,528 px |

All ink on the 967 pages is 538,606,321 px.

## Page by page against regions-cli

| Reader | Pages it covers better | Our gap there, median and most | Pages we cover better | Its gap there, median and most |
|---|---|---|---|---|
| bboxlog | 180 | 6 px, 414 px | 1 | 710,374 px |
| pdfplumber | 43 | 2 px, 150 px | 130 (49 files) | 28.5 px, 690,754 px |
| pymupdf | 69 | 3 px, 413 px | 120 (37 files) | 112 px, 785,193 px |

Where another reader does better, it's by a few pixels: on the 180 pages bboxlog wins, our missed ink is 0.0044% of their ink. These are the misses the tuning report lists (results/exact-tune.md, section 3): glyph ink past the advance width and capitals above the font's /Ascent, which MuPDF boxes from the glyph outline. Where we do better, a whole mark is missing from the other reader's list.

## What the other readers miss

- 003404 p0, all three: the page background is a rectangle filled with a coloured tiling pattern. PyMuPDF boxes the fill by its tile cell at the pattern origin, not the area painted (results/findings.md, checked on a one-path file). bboxlog 56.42%, pymupdf 51.83%; pdfplumber, which lists the rectangle, covers it.
- 003437, all 8 pages, pymupdf: the fonts' codes decode to control characters, which get_text("words") leaves out (results/findings.md). 14.88% to 44.74%. pdfplumber reads the same glyphs as "(cid:N)" with boxes shorter than the ink, 98.62% to 99.23%. bboxlog covers them.
- 003694 p1 and 003826 p2, pdfplumber: a shading painted with sh, which pdfminer.six doesn't report (axial, 297 x 758 pt and clipped, on 003694 p1, 36.55%; on 003826 p2, 97.13%, the page has one, but its misses weren't checked pixel by pixel). bboxlog gives both shadings an unbounded box (plus or minus 2^31), so it covers the whole page there whatever is drawn; these are the only two 003 pages with a shading.

## Phantoms

A baseline map has no flags, so its phantoms include marks regions-cli flags as white, hidden, offpage or invisible; they're not comparable one for one. bboxlog has 24,272 path, 1,484 word and 125 image phantoms, 24,001 of the paths on 003433, whose pages draw about 6,000 tiny tiles each; pdfplumber has 24,128 image and 3,088 word phantoms, pymupdf 24,130 image phantoms, both mostly the same tiles. regions-cli has 26 path, 121 word and 20 image phantoms, plus 217 white paths and 378 white words that it flags.

## Speed

Reader time a page, median, timing only the reader's calls on a page already loaded: bboxlog 2.19 ms, pymupdf 17.11 ms, pdfplumber 93.64 ms. regions-cli takes 1.10 ms a page median on 003 with the file parsed from bytes in memory (results/exact-tune.md, section 7), a different session, so this is an order of magnitude, not a close comparison.

## What this says

On completeness, MuPDF's own paint log and regions-cli are level on 003, apart from one tiling-pattern page where bboxlog loses half the ink and a few pixels of glyph overhang a page where regions-cli does. The test favours bboxlog: it is MuPDF reporting on MuPDF's own render. The calls people usually make, in PyMuPDF and in pdfplumber, miss whole marks on 37 and 49 files. So the exact layer's claim is matching MuPDF's own view of the page from an independent parser, with flags and drawing order on every region, in Rust and WebAssembly, rather than more coverage than MuPDF.

## After glyph outlines (GB1, 1 October)

regions-cli now adds each embedded glyph's outline to its word's ink box (D91, results/exact-heldout.md). On 003: 895 pages at 100% (from 787), all 967 at least 99.9%, worst 99.90%, missed ink 1,690 px (from 3,634), no page losing coverage. bboxlog still has more pages at 100% (966); the gap is now mostly text in fonts that aren't embedded, which MuPDF draws with its own substitutes.

## After fonts that aren't embedded (GB2, 2 October)

regions-cli now also boxes glyphs of fonts that aren't embedded by the standard font MuPDF draws instead (D93). On 003: 950 pages at 100%, all 967 at least 99.9%, worst 99.99%, missed ink 152 px, no page losing coverage. Against bboxlog (966 pages at 100%, worst 56.42%, 710,374 px missed) regions-cli now has fewer missed pixels, a better worst page and more pages at 99.9% or more; bboxlog still has 16 more pages at exactly 100%, most of them a few pixels short in embedded Type 1 fonts.

## After embedded Type 1 fonts (GB3, 2 October)

With Type 1 programs read as well (results/exact-heldout.md), regions-cli has 965 of 967 pages of 003 at 100%, the worst at 99.997%, and 35 missed pixels in all. bboxlog has 966 at 100% but misses 710,374 pixels and has a page at 56.42%.

## After the CFF and page-edge fixes (GB4 and GB5, 2 October)

regions-cli now covers every one of the 967 pages of 003 at 100%, with no missed pixel, against bboxlog's 966 pages at 100% and 710,374 missed pixels (results/exact-heldout.md). On this test it is now level with or ahead of all three other readers on every measure: pages at 100%, pages at 99.9% or more, the worst page, missed ink and unflagged phantoms.

## Notes

- The pdfplumber run above was made before the reader stopped dropping control-code characters as spaces (str.isspace() is true for them). Rerun on 003437's 8 pages after the change: identical numbers. Other pages weren't rerun.
- MuPDF printed the same 8 warnings in each run (6 "No default Layer config", 2 colour profile errors). No page failed.

## Reproduce

```
python tools/ink_baseline.py pymupdf real data/real/tune-003.jsonl <govdocs1>/003 --worst=12 --out=results/ink-003-base-pymupdf.jsonl
python tools/ink_baseline.py bboxlog real data/real/tune-003.jsonl <govdocs1>/003 --worst=12 --out=results/ink-003-base-bboxlog.jsonl
python tools/ink_baseline.py pdfplumber real data/real/tune-003.jsonl <govdocs1>/003 --worst=12 --out=results/ink-003-base-pdfplumber.jsonl
```
