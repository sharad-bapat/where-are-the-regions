# Has-text on scanned reports (5 October 2026)

Changes after the held-out run, frozen at 43e5fcd and run once on held-out data, reported apart from the original held-out numbers.

## The problem

After the fix in kinds-turn.md, what-needs-ocr's router still skipped every image on 695 of the 35,918 scanned pages in a 478-file sample of well reports from the Norwegian Offshore Directorate (Sodir). From a sample of 24 checked by eye, about 400 of those hold printed text. Looking at them turned up four causes in the kind layer, none of them a threshold set slightly wrong.

The first is polarity. On some scans the scanner's black margin covers just over half the image, and the kind layer took the majority side as the paper, so it read the page as white on black and counted the paper as ink. A typed depth table gave 47 components that way and 436 the right way round.

The second is long images. The thumbnail's long side was capped at 1,024 px whatever the shape, so a well log ten to twenty-five times taller than wide came out a hundred pixels across or less, and its text was gone.

The third is text printed sideways: tables turned a quarter on a portrait page, with their lines running up or down the image, where word runs were only looked for across it.

The fourth is sparse pages. A section divider ("SECTION A GEOLOGY", "4.7 Bit record") has a handful of marks in word runs, or none when the letters are spaced out, and a nearly empty page was cut off as blank before its letters were counted.

## The changes

All in regions/src/kind.rs and pixels.rs. When an image's dark share is between POLARITY_LO and POLARITY_HI (0.4 to 0.6), the ink is now whichever side makes more glyph-sized marks; outside that it's the majority, as before.

A thumbnail may now be longer than the limit, so that its short side keeps at least limit / ASPECT pixels (ASPECT is 2, so paper sizes are unchanged), up to LONG (8) times the limit (`pixels::long_side`).

Word runs are also counted with the glyph boxes turned a quarter each way (`turned_marks`), on the same ramp as photos (40 to 120 marks), since the uprights of charts and tables line up by chance. On 003, graphics without text had 17 to 77 turned marks; the sideways Sodir tables had 139 and 171.

Has-text also counts the share of glyph-sized marks on shared baselines (SPARSE_ALIGNED, 0.35 to 0.5) and how many there are (SPARSE_LINED, 5 to 10), but only on a page-sized, nearly empty image: ink under 1.5 to 3%, and a thumbnail whose long side is over 400 to 600 px. Without those two limits it held 2,202 more images in 003, mostly small inked logos and icons. A blank-looking page with such marks keeps the kind blank but gets has-text.

thumbs now prints the page each image is first drawn on, so pages can be scored.

## The Sodir set

320 scanned pages from the sample, picked before any of the changes (data/real/sodir-pages.jsonl, 1d4b26e). From pages with no text layer I took 120 where the router's has-text was under 0.1 and 40 where it was 0.1 or more, for a tune split and a held-out split each, split by file (90 and 84 files, none in both). Every file I'd looked at while finding the causes went to tune. The labels come from Tesseract with the same rules as the other image labels, one per page (tools/label_sodir.py). Each line of the page list gives the file's URL and sha256; the PDFs, and the words read from them, aren't redistributed. To rerun the scoring, download them into data/sodir, or point the SODIR environment variable at them.

Pages longer than 4,000 px at the label resolution are read in strips. The first 60 tune pages were labelled before that, and 8 well logs among them came back with no words when read whole. Read in strips, all 8 hold text, from 67 to 3,890 words, and one has a whole column of lithology descriptions. I relabelled all 13 long pages from that first batch before tuning finished. Only 3 of the 160 tune pages, and 3 of the 160 held-out pages, are labelled none, so the Sodir set measures finding text; holding text where there's none is measured on govdocs1.

## Results

Before is the committed code (62c1645, with thumbs' page output added so pages can be scored); after is the frozen code (43e5fcd). Tune results are in kinds-sparse-tune.txt, and the held-out run, done once, in kinds-sparse-heldout.txt.

| Measure | Before | After |
|---|---|---|
| Sodir tune: text pages found | 40 of 157 | 137 of 157 |
| Sodir tune: none pages not held | 3 of 3 | 3 of 3 |
| 003 has-text: text found | 303 of 383 | 305 of 383 |
| 003 has-text: none not held | 774 of 845 | 769 of 845 |
| Constructed tune: all targets | met | met |
| Sodir held-out: text pages found | 40 of 157 | 149 of 157 |
| Sodir held-out: none pages not held | 3 of 3 | 3 of 3 |
| 004 has-text: text found | 322 of 413 | 328 of 413 |
| 004 has-text: none not held | 651 of 716 | 642 of 716 |
| 004 kind: text labels called text | 240 of 381 | 240 of 381 |
| 004 kind: none labels not called text | 1,041 of 1,109 | 1,027 of 1,109 |
| Constructed held-out: text images found, others not called text | 179 of 180, 100 of 100 | 179 of 180, 100 of 100 |
| Kind at 0.9 and up, right (held-out) | 91.9% | 91.8% |
| Has-text at 0.9 and up, right (held-out) | 92.9% | 92.7% |

The before figure in the "low" stratum (pages the old code scored under 0.1) is 0 by construction, since that's how they were picked: 117 of each split's 157 text pages are in it.

On the scanned reports the change is large: held out, 149 of the 157 pages with text are found, against 40. On govdocs1 it costs a little. On 004, 6 more images with text are found, but 9 more without text are held and 14 more are called text. Both calibration targets are still met.

## Still open

The 8 held-out misses and 20 tune misses are mostly pages with one to three words, pages whose text is too small or faint at the thumbnail, and a few logs whose thumbnails are still under 50 px across. One of those draws its scan from inside a form XObject, which I haven't looked into. These pages still get read: what-needs-ocr's page floor sends the largest image of a page with no text of its own to OCR, whatever has-text says.
