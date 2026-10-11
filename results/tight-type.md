# Tight type (11 October 2026)

Some newspapers set their letters slightly overlapping and their word spaces at about 0.14 of the font size, with no space glyphs, so WORD_GAP (0.15) joined whole lines into one word: "hasstutteredinpreviously" on olmOCR-Bench's long_tiny_text 20_pg58. words() now looks at each run of words along one baseline: when the run has at least 6 gaps between glyphs inside words (TIGHT_MIN) and their median is below zero (overlapping letters), a gap more than 0.12 of the font size above that median (TIGHT_GAP) is a word break, unless it would cut off a single glyph or fall between two digits. On that line the gaps inside words are about -0.011 and the spaces 0.136.

Checks:

- wordbox's dev reference (659 files, 3,267,791 words that PyMuPDF and pdfplumber agree on): found went from 3,264,684 to 3,264,652 (99.90% either way), and pages with at least 99% found from 97.40% to 97.37% of 9,567. The reference tools join tight words too ("Faust,Michael", "JetPropulsionLaboratory"), so some of the 32 are splits they don't make. On the same files, 51 words were split, against 1,859 with a first version that didn't ask for overlapping letters (0.08 above the median), which cut "CONFIDENT IA L" and "manage r".
- The 1,113 files of the test sets: 1,062 identical; 51 differ, with 1,407 more words. In 10 of those the region figures that count words change with them (text_words, layer_words and their cover; one confidence 0.605 to 0.606), and nothing else.
- olmOCR-Bench dev slice, through doc-router: long tiny text +2 tests (both on the newspaper pages), every other category the same; 66.8 strict, from 66.3.

wordbox, which this word reader was copied from, is unchanged.
