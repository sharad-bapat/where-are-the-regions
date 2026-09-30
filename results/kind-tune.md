# Kind layer for images, tuning report (30 September 2026)

What regions/src/kind.rs says about each image from its own pixels, and how well, on tuning data only: the constructed tune split and govdocs1 003. Every image gets a kind (text image, photo, graphic, blank) with a confidence and reasons, and separately a has-text confidence (D86): a labelled chart is a graphic that holds text. Neither is part of regions-cli's output yet; tools/score_kinds.py reads them from the thumbs tool (`thumbs - --kinds`). The exact layer's output is unchanged (tools/diff_exact.py, 1,113 files identical after every change).

## Pixels

regions/src/pixels.rs decodes each image XObject to a grey thumbnail (at most 1,024 px on the long side for the kind): Flate, LZW and the other stream filters, PNG and TIFF predictors, JPEG through zune-jpeg 0.5.15, CCITT fax through our own decoder (regions/src/ccitt.rs). Checked against PyMuPDF's decode of the same objects under the same grey formula: 3,906 images on 40 files of 003 within 1 grey level (JPEG included), and 5,162 fax images on 39 files within half a level. JPX and JBIG2 aren't decoded (D84), nor Indexed over DeviceN (003074) or 003209's predictor rows; inline images have no object and are counted apart.

## Measures

From the thumbnail: the spread and count of grey levels, the share near black or white, the ink share, connected components of the ink, the glyph-sized ones (2 to 60 px tall), their height spread, the share on shared baselines, and word-like runs (3 or more glyph-sized marks of similar height, each within about a height of the next on one baseline).

## Constructed tune split

| Case kind | Kind given |
| --- | --- |
| text, text_ocr, full_scan (157) | text 157 |
| photo (13) | photo 13 |
| logo (17) | graphic 14, blank 3 |
| blank, background, rule (55) | blank 55 |

Text images found 157 of 157 and other images not called text 85 of 85 (targets 95% each, D84). Has-text held on all 157 text images and on none of the 68 photos, blanks, backgrounds and rules. 38 inline images are counted apart.

## govdocs1 003

Kind against the region labels (tools/select_real.py): text labels called text 218 of 354; none labels not called text 951 of 1,001. Most text labels given another kind are charts and logos with labels (a graphic that holds text, which has-text answers) and images with text drawn over them by other marks (results/handcheck-kinds-003.md).

Has-text against labels from each image's own pixels (tools/label_images.py, data/real/tune-003-image.jsonl; unsure counts as text, D67): images with text held 300 of 376 (79.8%), images without not held 770 of 844 (91.2%). 93 aren't placed or decoded and are counted apart. (A first labelling run failed on 17 Separation-colour images; tools/label_images.py now converts them, and they're labelled.)

## Calibration

Share right by confidence band, both sets together (target: at 0.9 or more, at least 90% right):

| Band | Kind | Has-text |
| --- | --- | --- |
| 0.9 and up | 1,061 of 1,126 (94.2%) | 1,196 of 1,295 (92.4%) |
| 0.7 to 0.9 | 91 of 130 (70.0%) | 54 of 78 (69.2%) |
| 0.5 to 0.7 | 97 of 129 (75.2%) | 45 of 72 (62.5%) |
| under 0.5 | 162 of 212 (76.4%) | none |

## Rules chosen, and where

The text kind leans on the baseline share (ramp 0.5 to 0.75), set in the gap on the constructed split (text images 0.66 and up, logos 0, photos at most 0.57). Has-text ramps on word-like marks are centred on 003's best single cut per kind: about 24 for graphics, 80 for photos (texture makes short runs), 17 for text images. The hand check found the 003 labels miss small text often (13 of 40 "none" images with text evidence did hold text), so these numbers undercount has-text's real accuracy on text.

## Not yet done

Held-out runs (constructed heldout, and govdocs1 004 with its own image-only labels) come with 8f, after the kind layer is frozen. Inline images need the parser to keep their data (asked about; reported apart for now).
