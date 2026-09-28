# Ink test, baseline (28 September 2026)

What share of each page's ink does the map cover today, before vector paths, annotations and better text boxes are added? Measured with `tools/ink_check.py`, which renders each page with PyMuPDF at 150 dpi and counts pixels darker than 250 on any channel. The map is the images and the visible words from regions-cli, each box grown by 1 pt for anti-aliasing. Missed ink is put down to whatever PyMuPDF finds under it, in the order text, image, vector, annotation, other.

Only tuning data is used: the constructed tune split and govdocs1 003. The held-out sets are untouched.

## Coverage

| Set | Pages | Fully covered | At least 99.9% | At least 99.5% | Median | Worst |
|---|---|---|---|---|---|---|
| Constructed tune | 300 | 199 | 232 | 241 | 100.00% | 66.49% |
| govdocs1 003 | 967 | 325 | 440 | 480 | 99.46% | 10.31% |

## Missed ink by kind

| Set | Text | Image | Vector | Annotation | Other |
|---|---|---|---|---|---|
| Constructed tune, share of all ink | 0.042% | 0 | 0.558% | 0 | 0 |
| Constructed tune, pages | 87 | 0 | 66 | 0 | 0 |
| govdocs1 003, share of all ink | 0.578% | 0.425% | 7.223% | 0 | 0.697% |
| govdocs1 003, pages | 468 | 75 | 534 | 0 | 128 |

Phantoms, regions whose own box holds no ink: none of the images and 3 words on the constructed set; 23 images and 794 words on 003.

## What the misses are

Vector paths are most of it, as expected: the parser doesn't track them yet. One file, 003161, accounts for 14 of the 25 worst pages, each a chart or map drawn almost entirely with paths.

Text misses come from the word box, not from missing words. A word's box runs from one em above the baseline down to the baseline, so descenders and underscores fall outside it. On constructed case c0148 every PyMuPDF word overlaps one of ours, but lines of 20 pt underscores draw their ink below our boxes, which also makes them the 3 constructed phantoms. The fix is to take the box from the font's ascent and descent.

Most phantom words on 003 are text hidden under an image. On 003431 page 5 the whole page is a scan, and its text is drawn in the normal render mode before the scan paints over it. The word boxes land near the scan's own letters but not on them. Drawing order will show this directly.

Image and other misses on 003 are still to be looked at. The earlier comparison with PyMuPDF found that it reports soft-mask images and shadings that aren't placed images, so some of the image share may be shadings, and shadings would also explain some of the other share.

## Next

Chunk 3 builds text regions from font extents and adds drawing order and visible boxes. Chunk 4 adds vector paths and shadings. The ink test is rerun after each.

## Reproduce

```
python tools/ink_check.py constructed tune --worst=12
python tools/ink_check.py real data/real/tune-003.jsonl <govdocs1>/003 --worst=25 --out=ink-003.jsonl
```

MuPDF printed 38 warnings while rendering 003 (35 "No default Layer config", 3 colour profile errors). No page failed.
