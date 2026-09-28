# Ink test, baseline (28 September 2026)

What share of each page's ink does the map cover today, before vector paths and annotations are added? Measured with `tools/ink_check.py`, which renders each page with PyMuPDF at 150 dpi and counts pixels darker than 250 on any channel. The map is the images and the visible words from regions-cli, each box grown by 1 pt for anti-aliasing. Missed ink is put down to whatever PyMuPDF finds under it, in the order text, image, vector, annotation, other.

Only tuning data is used: the constructed tune split and govdocs1 003. The held-out sets are untouched.

These numbers are after the descent fix (below) and two fixes to the test's own attribution. The first run's coverage was the same to within two pages; its breakdown by kind was wrong on rotated pages.

## Coverage

| Set | Pages | Fully covered | At least 99.9% | At least 99.5% | Median | Worst |
|---|---|---|---|---|---|---|
| Constructed tune | 300 | 200 | 233 | 242 | 100.00% | 66.49% |
| govdocs1 003 | 967 | 327 | 440 | 482 | 99.48% | 10.31% |

## Missed ink by kind

| Set | Text | Image | Vector | Annotation | Other |
|---|---|---|---|---|---|
| Constructed tune, share of all ink | 0.005% | 0 | 0.575% | 0 | 0 |
| Constructed tune, pages | 28 | 0 | 90 | 0 | 9 |
| govdocs1 003, share of all ink | 0.345% | 0.175% | 8.253% | 0 | 0.149% |
| govdocs1 003, pages | 364 | 16 | 558 | 0 | 185 |

Phantoms, regions whose own box holds no ink: none on the constructed set; 23 images and 767 words on 003.

## What the misses are

Vector paths are most of it, as expected: the parser doesn't track them yet. One file, 003161, accounts for 14 of the 25 worst pages, each a chart or map drawn almost entirely with paths.

The text share is mostly not missed text. A pixel is put down to text when it lies inside one of PyMuPDF's character boxes, and those are a little taller than ours, so a coloured background showing around the letters counts as text. On the 12 pages with the most text-attributed ink, every word PyMuPDF finds overlaps one of ours, and on 003704 (a dark blue cover) the boxes match to 0.1 pt. The test can only tell text from background once vector fills are in the map.

Most phantom words on 003 are text hidden under an image. On 003431 page 5 the whole page is a scan, and its text is drawn in the normal render mode before the scan paints over it. The word boxes land near the scan's own letters but not on them. Drawing order will show this directly.

The image and other shares are still to be looked at. The earlier comparison with PyMuPDF found that it reports soft-mask images and shadings that aren't placed images, so shadings may explain both.

## Fixes found by the test

Glyph boxes stopped at the baseline for fonts whose descriptor gives a positive /Descent. The spec says it's negative, but ContractNLI and govdocs1 files carry "/Descent 270" for Arial Unicode and 206 for Tahoma, and the parser clamped it to 0. The descent's sign is now forced, and a zero descent falls back to /FontBBox. On c0148 a line of 20 pt underscores now has the same box as PyMuPDF's, and the constructed set's 3 phantom words are gone.

The test itself had two faults. PyMuPDF gives text, image and drawing boxes on the unrotated page but renders the page turned, so on rotated pages its boxes are now mapped through the page's rotation matrix. And spaces have character boxes, so background showing between words was counted as text; spaces are now skipped.

## Next

Chunk 3 adds drawing order and visible boxes to every region and moves the OCR scoring into its own module. Chunk 4 adds vector paths and shadings. The ink test is rerun after each.

## After vector paths (chunk 4a)

Painted paths and shadings are now in the map: each path's box through the transform (curves by their real extent, strokes grown by half the line width), clipped to its clip, and touching paths joined into one region. The ink test counts each path's own box, not the joined region's.

| Set | Pages | Fully covered | At least 99.9% | At least 99.5% | Median | Worst |
|---|---|---|---|---|---|---|
| Constructed tune | 300 | 288 | 300 | 300 | 100.00% | 99.99% |
| govdocs1 003 | 967 | 755 | 959 | 967 | 100.00% | 99.77% |

Missed ink is now 0.002% of all ink on 003, on 112 pages as text and 159 as other, a few hundred pixels at most. The worst pages (003441, 003186) have phantom words as well, so the misses there look like text drawn a little outside its box; that's next to look at, with the clip work.

Phantom paths: 16 white paths on the constructed set and 190 on 003 have no ink, as expected on a white page. Another 38 and 39 non-white paths have none either. Some are likely painted in a colour space the parser doesn't read (a Separation tint of 0 is white), and some covered by later paint.

After clips (chunk 4b), every glyph, image and path is cut to the clip it was drawn with, and one clipped away entirely is flagged hidden. Coverage didn't move (755 and 288 pages fully covered), so the cuts remove no ink the renderer draws; 3 pages on 003 now miss a few image pixels at a clip's edge, under 0.001% of ink. The worst text misses left (003441) are Symbol-font glyphs the parser can't measure, not clipping.

Speed on all 278 files of 003, one run each on a quiet machine: median 7.9 ms a file before paths, 8.7 ms after.

## Reproduce

```
python tools/ink_check.py constructed tune --worst=12
python tools/ink_check.py real data/real/tune-003.jsonl <govdocs1>/003 --worst=25 --out=ink-003.jsonl
```

The constructed run takes about 3.5 minutes and 003 about 10. MuPDF prints 38 warnings while rendering 003 (35 "No default Layer config", 3 colour profile errors). No page failed.
