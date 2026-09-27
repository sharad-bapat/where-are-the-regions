# Hand check of the Tesseract labels, govdocs1 thread 003 (tuning)

Second check, 28 September 2026, after fixing rotated pages in tools/select_real.py (boxes are now in displayed page coordinates, and redaction covers the whole rotated page) and counting words of 2 or more digits (D67). Sample: 20 regions per label, seed 20260927, by tools/sample_sheet.py, looked at by eye.

| Label | Checked | Right | Notes |
| --- | --- | --- | --- |
| text | 20 | 20 | charts with axis labels, a table of contents, figure captions, logos |
| none | 20 | 17 | #20 small lettering on a picture of a plane, #24 a mirrored scale bar on a micrograph, #28 a yellow annotation on brain scans |
| unsure | 20 | 20 hold text | logos (Fitch, NNSA, USDA, NTNU, Los Alamos), a table, axis labels, a calculator screen |

So "unsure" is text, and "none" misses about 1 in 7: small, mirrored or coloured text Tesseract doesn't read with confidence. For scoring, unsure counts as text (D67), and the none misses are a known undercount of the reference.

The first check (27 September, before the fix) found text 20 of 20, none 18 of 20, unsure about 17 of 20 text; its boxes on rotated pages were wrong.

Counts (tools/select_real.py, max 30 pages a file):

| Thread | Pages | Regions | text | none | unsure | under a text layer |
| --- | --- | --- | --- | --- | --- | --- |
| 003 (tune) | 967 | 1,875 | 377 | 1,228 | 270 | 0 |
| 004 (held out) | 704 | 1,904 | 394 | 1,154 | 356 | 5 |

Thread 004 was labelled but not looked at. On 003, regions-cli has a matching image for 1,869 of 1,875 regions, with no rotation fix-up needed now.
