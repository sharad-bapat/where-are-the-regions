# Rules and glyph fonts (7 October 2026)

An addition to the output after the held-out run. Nothing that was in the output before changes.

## Why

Two things a caller builds from a page need the lines it draws and the font of each glyph, and neither was in the output. A ruled table's cells are the rectangles its rules enclose, the way pdfplumber and Camelot's lattice mode find them, but the map gives vector paths as clusters of touching paths: a table grid drawn as one path, or as many touching ones, comes out as a single box. And a fraction bar in typeset maths is a short drawn rule, while the maths alphabets (blackboard bold, calligraphic, the italic that tells \phi from \varphi) are known only from each glyph's font. The words carried a font, but a word can mix fonts, and the glyphs didn't.

## What

Each page now has `rules`: the straight level and upright segments it draws, as `[x0, y0, x1, y1]` on the displayed page. They come from where each painted path is recorded (regions/src/lib.rs, `rules_of` and `place_rules`):

- every line segment of a stroked path that is level or upright within RULE_SKEW (0.5 points) and at least RULE_MIN (4 points) long;
- the edges of rectangles drawn with `re`, stroked or filled, since a filled cell's edges bound it too;
- for a filled bar no thicker than RULE_BAR (2.5 points), its centre line instead of its edges.

A path painted only in white gives no rules. Each rule is cut to its clip and to the page, dropped if that leaves it under RULE_MIN, and repeats are dropped.

With `--glyphs`, each glyph now has `f`, the index of its font in `fonts` (-1 when no font was set), as words already had.

## Check

I built the last commit's source on the same pdf-core and ran it and the new build with `--glyphs` on the 1,113 files of the test sets (the constructed tune and held-out sets, govdocs1 003 and 004). After taking out each run's time, each page's `rules` and each glyph's `f`, the two outputs were identical on all 1,113 files. They hold 388,715 rules.

The 81 library tests pass. The browser build hasn't been rebuilt with these fields yet.

Relocked at pdf-core a9328cd, which gives text to glyphs from TeX's maths fonts that had none (its README, "Maths glyph names"). On these 1,113 files, wordbox, which decodes glyphs through the same pdf-core code, gained text on 1,610 words in 19 files and changed nothing else. This repo's own words weren't compared across that pdf-core change.
