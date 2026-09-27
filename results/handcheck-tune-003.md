# Hand check of the Tesseract labels, govdocs1 thread 003 (tuning)

Sample: 20 regions per label, seed 20260927, drawn by tools/sample_sheet.py and looked at by eye, 27 September 2026.

| Label | Checked | Agree | Notes |
| --- | --- | --- | --- |
| text | 20 | 20 | every tile has text in its pixels; many are small (chart labels, logos, banners) |
| none | 20 | 18 | #28 shows clipped words; #31 is a chart whose labels are only digits, which the 3-letter filter drops |
| unsure | 20 | about 3 are really none | about 17 hold real text: logos, axis labels, slide fragments |

So the labeller undercounts text. It misses stylised and small text, and text made only of digits. "unsure" is mostly text.

Open for the user before scoring: does a logo or a chart's axis labels count as "needs OCR", or only running text? The answer sets how "unsure" is resolved (hand-label every one, or a second reader).

Counts (tools/select_real.py, max 30 pages a file):

| Thread | Files with a qualifying page | Pages | Regions | text | none | unsure | under a text layer |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 003 (tune) | 155 of 278 | 962 | 1,829 | 351 | 1,267 | 211 | 0 |
| 004 (held out) | 111 of 235 | 698 | 1,769 | 339 | 1,082 | 348 | 5 |

Thread 004 was labelled but not looked at.
