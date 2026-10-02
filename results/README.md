# Results

Every number in the top-level README comes from a file here. Reports are Markdown; the .jsonl and .txt files are the raw output they were written from.

## Reports

| File | What it reports |
|---|---|
| exact-tune.md | The exact layer on tuning data, the constants and where they came from, speed on 30 September |
| exact-heldout.md | The exact layer's held-out run, then each fix made after it and its effect (Symbol and Type 3 fonts, glyph outlines GB1 to GB5) |
| ink-baseline.md | The ink test's first baseline, and the runs as vector paths, clips and the standard-font tables went in |
| ink-baseline-readers.md | The same ink test on PyMuPDF and pdfplumber boxes |
| kind-tune.md | The image kinds on tuning data |
| kind-heldout.md | The image kinds' held-out run, and the striped-image fix after it |
| vkind-heldout.md | The vector kinds' held-out run, which missed its calibration target |
| speed.md | Native and WebAssembly timings on all of 003 and 004 |
| findings.md | Facts found about other software and real PDFs, with the evidence |
| handcheck-tune-003.md | A hand check of the Tesseract labels used for 003 |
| handcheck-kinds-003.md | A hand check of image-kind disagreements on 003 |

## Raw output

| File | From |
|---|---|
| ink-003-*.jsonl, ink-004-*.jsonl | tools/ink_check.py, one line per page, after the chunk named in the file name (post = after the held-out fixes, gb1 to gb5 = the glyph-bounds chunks) |
| ink-003-base-*.jsonl | tools/ink_baseline.py, one per reader |
| ink-constructed-tune.jsonl, ink-constructed-heldout.jsonl | tools/ink_check.py on the constructed set |
| marks-heldout.txt, regions-heldout.txt | tools/score_map.py and tools/score_regions.py on the held-out splits |
| kinds-heldout.txt, kinds-heldout-post.txt | tools/score_kinds.py, the held-out run and after the strip fix |
| vkinds-heldout.txt | tools/score_vkinds.py --heldout |
| frozen.sha256 | the frozen files and their hashes, checked by tools/check_frozen.py |
