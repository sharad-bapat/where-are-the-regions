# Kind layer for images, held-out results (30 September 2026)

The kind layer (regions/src/kind.rs, with pixels.rs and the decoders) measured once on data it was never tuned on, after the freeze (D88, commit bcbc5d7; tools/check_frozen.py passed first). The targets were fixed before (D84). Output: results/kinds-heldout.txt. Tuning results are in results/kind-tune.md.

## Summary

| Target (D84) | Result | |
| --- | --- | --- |
| Constructed heldout: text images found, at least 95% | 147 of 150 (98.0%) | met |
| Constructed heldout: photos, logos, blanks, backgrounds, rules not called text, at least 95% | 88 of 88 (100%) | met |
| Kind: at confidence 0.9 or more, at least 90% right | 1,025 of 1,122 (91.4%) | met |
| Has-text: at confidence 0.9 or more, at least 90% right | 1,104 of 1,185 (93.2%) | met |

## Constructed heldout split

| Case kind | Kind given |
| --- | --- |
| text (90) | text 88, blank 2 |
| text_ocr (40) | text 40 |
| full_scan (20) | text 19, graphic 1 |
| photo (16) | photo 16 |
| logo (16) | graphic 12, blank 4 |
| blank, background, rule (56) | blank 56 |

Has-text held on 147 of 150 text images and on none of the 72 photos, blanks, backgrounds and rules; no logo held. 42 inline images are counted apart. All three text misses (c0275, c0331, c0393) are striped images: the page draws the picture as several strips, which the map merges into one region, but the kind is judged on the largest single strip, and in c0275 and c0393 that strip is a blank band.

## govdocs1 004

Has-text against labels from each image's own pixels (tools/label_images.py, data/real/heldout-004-image.jsonl, 1,445 images, no labelling errors; unsure counts as text, D67): images with text held 313 of 401 (78.1%), images without not held 657 of 719 (91.4%). On 003 the same numbers were 79.8% and 91.2%.

Kind against the region labels: text labels called text 215 of 348; none labels not called text 821 of 846. As on 003, text labels given another kind are mostly charts and figures with labels (has-text answers those) and images with text drawn over them.

Counted apart: 318 labelled images the map doesn't place as their own region (103 on 004651, 88 on 004989, 22 on 004440), most likely strips merged into another image's region, as in the constructed misses above; not checked file by file. 7 JPX images.

## Calibration

| Band | Kind | Has-text |
| --- | --- | --- |
| 0.9 and up | 1,025 of 1,122 (91.4%) | 1,104 of 1,185 (93.2%) |
| 0.7 to 0.9 | 58 of 79 (73.4%) | 50 of 89 (56.2%) |
| 0.5 to 0.7 | 58 of 72 (80.6%) | 35 of 68 (51.5%) |
| under 0.5 | 130 of 159 (81.8%) | none |

## After the held-out run

One change after the numbers above were recorded (8g, D89): a merged image (strips) is now judged from all its pieces pasted onto one canvas (regions/src/pixels.rs merged_thumbnail; the map keeps every piece, lib.rs Image.pieces, not in the JSON), and its truth on govdocs1 is whole too: it holds text when any of its strips is labelled text or unsure, and it counts once (tools/score_kinds.py). regions-cli's output is unchanged (tools/diff_exact.py). Relocked, then measured the same way (results/kinds-heldout-post.txt):

| | Held-out run | After 8g |
| --- | --- | --- |
| Constructed heldout: text images found | 147 of 150 (98.0%) | 149 of 150 (99.3%) |
| Constructed heldout: has-text on text images | 147 of 150 | 150 of 150 |
| Constructed heldout: others not called text | 88 of 88 | 88 of 88 |
| govdocs1 004: images not placed | 318 | 4 |
| govdocs1 004 has-text: images with text held | 313 of 401 (78.1%) | 323 of 413 (78.2%) |
| govdocs1 004 has-text: images without not held | 657 of 719 (91.4%) | 652 of 716 (91.1%) |
| Kind at 0.9 or more, right | 91.4% | 91.7% |
| Has-text at 0.9 or more, right | 93.2% | 92.7% |

(After 8g the govdocs1 counts are merged images, not strips.) On 003 (tune) the same change places every labelled image: has-text holds 304 of 383 images with text and leaves 771 of 845 without; calibration at 0.9 or more is 93.2% for the kind and 92.2% for has-text, on both tune sets together.

## Inline images (3 October 2026)

After the held-out run, and relocked on its own: the map now keeps an inline image's dictionary and data (the bytes between BI and ID, and between ID and EI), and regions/src/pixels.rs decodes them through the same filters and colour spaces as an image XObject, with the short keys (/W, /H, /CS, /BPC, /F and the rest) spelt out (pixels::inline_thumbnail; a colour space named from the page's resources isn't looked up yet). The thumbs tool and the WebAssembly kinds give inline images a kind, keyed -1, -2 in drawing order since they have no object number. The map's JSON doesn't change: old and new output are identical on all 1,553 files of 003, 004 and the constructed sets. Wanted by the next tool, which has to tell an inline photo from an inline scan.

| Constructed set | Before (inline counted apart) | With inline images |
|---|---|---|
| Tune: text images found | 157 of 157 | 180 of 180 |
| Tune: other images not called text | 85 of 85 | 100 of 100 |
| Held-out: text images found | 149 of 150 | 179 of 180 (99.4%) |
| Held-out: other images not called text | 88 of 88 | 100 of 100 |
| Held-out: has-text on text images | 150 of 150 | 180 of 180 |

No image is counted apart any more on either split. The one held-out miss is the full-page scan already missed before, called a graphic. The govdocs1 labels match images by object number, so inline images there are still left out of the real-page scores.
