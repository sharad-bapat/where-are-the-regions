# Hand check of the image kinds against the Tesseract labels, govdocs1 003 (tuning)

30 September 2026, chunk 8e (D86). The kind layer (regions/src/kind.rs, first tuning round) and the 003 labels (tools/select_real.py: Tesseract, confidence 60 or more, on each placement's box rendered with the page's digital text removed) disagree on about 370 placements. A seeded sample of each disagreement was looked at by eye: 40 labelled none that show text evidence (8 or more glyph-sized marks, at least half on shared baselines), and 40 labelled text that don't. Seed 20260930, tiles rendered at 110 dpi as the labeller saw them.

| Group | Checked | Hold text | No text |
| --- | --- | --- | --- |
| A: label none, text evidence | 40 | 13 | 27 |
| B: label text, no text evidence | 40 | 40 | 0 |

Group A. The 13 with text are small labels Tesseract didn't read with confidence: a diagram's symbols, axis numbers on four plots and graphs, two maps, two posters, a photo's caption, four agency seals (circular lettering), "4100A" on an excavator, numbers on padlocks. The 27 without are photos, murals, 3D renders, line drawings and micrographs whose texture forms glyph-sized marks that happen to share rows. So the labels undercount text (as the 28 September check found for none labels, about 1 in 7 there), and the evidence rule also fires on busy pictures.

Group B. All 40 hold text, so the labels were right. Two causes:

- 11 are placements whose own pixels hold no text: the text is drawn over the image by other marks. 003298's tube maps and line diagrams sit on a flat dark-blue JPEG background (object 168, placed many times), and 003823's NTNU logo region is a one-colour image mask with the lettering drawn separately. The label describes the rendered area; the kind describes the image's pixels, and "blank" is right about those.
- 29 are charts, tables and maps with text in the image: axis numbers, legends, place names, table cells. They have 39 to 344 glyph-sized marks but only 12% to 49% of them on shared baselines, because chart markers (circles, crosses) are glyph-sized too and vertical axis numbers don't share a baseline. The share of aligned marks is the wrong measure there; a count of marks in word-like runs would be better.

What follows (D86): the kind stays a description of the image's pixels, and a separate has-text confidence is added. Scoring has-text against these labels needs the overlay cases set apart (or a reference taken from the image's own pixels), and "unsure" counts as text (D67).
