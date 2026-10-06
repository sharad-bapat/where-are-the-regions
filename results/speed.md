# Speed (2 October 2026)

How long the page map takes, natively and in the browser build, on every page of govdocs1 threads 003 (tune) and 004 (held out), measured in one session after the glyph-bounds work (GB1 to GB5) and the WebAssembly build (commit 2ba248c). This replaces the timings in results/exact-tune.md section 7 and the noisy ones taken during GB1 to GB5 as the numbers to quote.

## Method

- Native: tools/speed.py runs regions-cli (release build) over every file, three passes, and takes each file's median of regions-cli's own "micros": the time of extract() on bytes already in memory, so reading the file from disk isn't counted.
- Browser build: tools/wasm_speed.mjs runs wasm/pkg's extract_json in Node 24, three passes, each file's median, timed around the call alone.
- A page's time is its file's time over the file's pages. All pages of every file are counted, not only the labelled ones.
- Both on the same laptop, one after the other. Timings on this machine drift between sessions (exact-tune.md section 7), so only numbers from one session compare.

## Results

| Set | Build | Files | Pages | A page, median | 90th percentile | A file, median | All files |
|---|---|---|---|---|---|---|---|
| 003 | native | 278 | 7,633 | 2.03 ms | 5.17 ms | 22.5 ms | 27.5 s |
| 003 | wasm | 278 | 7,633 | 1.90 ms | 5.00 ms | 23.6 ms | 29.8 s |
| 004 | native | 235 | 4,211 | 1.69 ms | 5.33 ms | 16.9 ms | 118.4 s |
| 004 | wasm | 235 | 4,211 | 2.55 ms | 8.27 ms | 28.3 ms | 175.7 s |

The target set before the held-out runs (D76), a median under 5 ms a page natively, is met on both threads.

The totals are set by a few files. On 004, one file (004991, 76 pages, about 1.3 s a page natively) takes most of the 118 seconds; on 003 the slowest page is 003190's single page at 1.5 s. Neither has been profiled yet.

The browser build runs at about the native speed at the median and is slower in the tail (004's total is about 1.5 times the native one). Its output is identical to the native build's on all 1,553 test files (tools/wasm_check.mjs). It is 772,557 bytes, 315,451 gzipped. Later rebuilds are below.

For scale, the reader comparison (results/ink-baseline-readers.md) timed PyMuPDF's get_bboxlog at 2.19 ms a page median on 003, with the file already open and parsed, and pdfplumber at 93.6 ms.

## Reproduce

```
python tools/speed.py <govdocs1>/003 <govdocs1>/004
node tools/wasm_speed.mjs <govdocs1>/003 <govdocs1>/004
```

## Thumbnails (5 October 2026)

On scanned files the time went on decoding image pixels for the kind layer: on a 93-page file of 1-bit scans from the Norwegian Offshore Directorate, `thumbs --kinds` took 10.0 s, against 0.2 s for the page map. Two changes fixed most of it without changing the output. The file is now indexed once for all its thumbnails (`pixels::Source`), where it used to be indexed again for every image. And one-component images of up to 8 bits (scans, grey, masks) are averaged through a table of grey levels made once per image, with 1-bit rows taken a byte at a time.

The same file now takes under 2 s. Over the 866 files of the kind sets (the constructed tune and held-out sets and the labelled files of govdocs1 003 and 004, 38,762 images), `thumbs --kinds` took 163 s against 334 s, and its output was byte for byte the same. what-needs-ocr's router took 39 s on 15 Sodir files, against 188 s.

## WebAssembly rebuilds

Each rebuild was checked against the native build with tools/wasm_check.mjs:

| Rebuilt with | Size (gzipped) | Output identical on |
|---|---|---|
| the width fix and inline-image pixels (5 October 2026) | 814,274 bytes (330,152) | the 300 constructed tune pages |
| thumbnails turned to the page (8019e6d) | 815,615 bytes (331,470) | 26 files: the demo samples, 20 constructed held-out pages, two rotated scans |
| the has-text changes (results/kinds-sparse.md) | 821,098 bytes (333,537) | 1,113 files: the constructed tune and held-out sets and govdocs1 003 and 004, with all 49,724 vector kinds |

The last check didn't include the marks and vectors sets that made up the original 1,553 files.
