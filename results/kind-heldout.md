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

Nothing has been changed since. The strip issue is the clear next fix: judge a merged image by all its strips (the thumbnails of each part placed side by side), not by the largest part. It would go in as its own chunk, with a new freeze, reported apart from these numbers (D81).
