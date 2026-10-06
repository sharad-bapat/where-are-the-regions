# Images judged as they show on the page (5 October 2026)

A fix after the held-out run, reported apart from it. Commit 8019e6d, relocked.

## The problem

The kind layer judged each image from its stored pixels. A scan stored on its side and turned by the page's /Rotate had its lines of text running down the thumbnail, and has-text counts marks along rows, so it found none. Striped scans on such a page were worse. Each strip's resolution was taken along the wrong side of its box, so a 3504 x 2480 scan made of 9 CCITT strips came out as a 57 x 80 thumbnail with no letters left in it.

I found it on scanned well reports from the Norwegian Offshore Directorate (Sodir). In the first 10 files of a 478-file sample, what-needs-ocr's router skipped 323 of 1,105 image regions as holding no text. I looked at two of the skipped pages, a cover (/Rotate 270, in strips) and a page of prose (/Rotate 90, one image). Both are plain text, and both had has-text 0.05.

## The fix

Each drawn piece of an image now records which way its stored pixels face on the displayed page, the page's /Rotate and the image's own matrix taken together, to the nearest quarter turn (`regions/src/pixels.rs`, `Turn`; `lib.rs`, `Piece`). The thumbnail is turned to match before it's judged, and before strips are pasted together. An upright image comes back untouched, byte for byte. Thumbnails now come from `pixels::Source::placed`, which thumbs, the WebAssembly build and the router all use (`placed_thumbnail` does the same for a single image).

On the two Sodir pages has-text went from 0.05 to 0.95 (word marks 0 to 644 on the prose page, 0 to 113 on the cover). On the same 10 files, skipped regions went from 323 to 17.

## Effect on the test sets

I ran `thumbs --kinds` over the constructed tune and held-out sets and the labelled files of govdocs1 003 and 004 (866 files, 38,762 images) before and after. 877 images changed, all in 003 and 004 and none in the constructed sets. Their thumbnail sizes didn't change, so they're images drawn upside down or mirrored by their own matrix, now read the right way round.

Scores before (the committed source) and after:

| Measure | Before | After |
|---|---|---|
| 003 has-text: text found | 304 of 383 | 303 of 383 |
| 003 has-text: none not held | 771 of 845 | 774 of 845 |
| 003 kind: text labels called text | 227 of 374 | 231 of 374 |
| 003 kind: none labels not called text | 1,066 of 1,143 | 1,066 of 1,143 |
| 003 kind at 0.9 and up, right | 910 of 994 (91.5%) | 912 of 999 (91.3%) |
| 003 has-text at 0.9 and up, right | 979 of 1,081 (90.6%) | 986 of 1,090 (90.5%) |
| Constructed held-out: text images found | 179 of 180 | 179 of 180 |
| Constructed held-out: others not called text | 100 of 100 | 100 of 100 |
| 004 has-text: text found | 323 of 413 | 322 of 413 |
| 004 has-text: none not held | 652 of 716 | 651 of 716 |
| 004 kind: text labels called text | 239 of 381 | 240 of 381 |
| 004 kind: none labels not called text | 1,043 of 1,109 | 1,041 of 1,109 |
| 004 kind at 0.9 and up, right | 1,257 of 1,367 (92.0%) | 1,255 of 1,366 (91.9%) |
| 004 has-text at 0.9 and up, right | 1,148 of 1,236 (92.9%) | 1,146 of 1,234 (92.9%) |

No measure moved by more than four images. govdocs1 has few rotated scans, so the fix matters on files like Sodir's and hardly shows here. The full held-out output is in kinds-heldout-turn.txt.

Of the 17 regions still skipped in the 10 Sodir files, at least four held text: three section-divider pages with a line or two of type, and a dense summary sheet with a map. Those led to the next set of changes, in kinds-sparse.md.
