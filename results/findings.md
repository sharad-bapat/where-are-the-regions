# Findings about other software and real files

Facts the checks turned up about libraries, data sources and PDFs in the wild, as opposed to bugs in this repo's own code. Each entry says how it was found and what it was checked against.

## ttf-parser rejects a CFF glyph that uses dotsection (2 October 2026)

ttf-parser 0.25.1 fails to outline any CFF charstring containing the escape operator 12 0 (dotsection): under the escape byte it accepts only hflex, flex, hflex1 and flex1 and returns CFFError::UnsupportedOperator for the rest (src/tables/cff/cff1.rs, parse_char_string), so the glyph has no outline and no box. Adobe's Type 2 Charstring Format (TN 5177, 16 March 2000, Appendix C, p35) says the obsolete and deprecated operators "should be supported in all Type 2 charstring processors that may encounter such programs", and that dotsection "has always been treated as a no-op by Adobe ATM renderers"; the operator table (p32) marks 12 0 as deprecated. FreeType, and so MuPDF, draws these glyphs. Found on govdocs1 003161 p1, JansonText-Italic (FontFile3 /Type1C), where 8 of 59 non-empty glyphs fail, 'j' among them (fontTools reads its control box as -202 -270 274 690). Counted with fontTools over every FontFile3 /Type1C and /CIDFontType0C program: 36 files of thread 003 (681 programs, 647 of 30,088 charstrings and subroutines) and 24 of thread 004 (554 programs, 395 of 15,902) use it.

## pdfminer.six ignores the sh operator, so pdfplumber lists no shadings (1 October 2026)

In pdfminer.six 20251230, PDFPageInterpreter.do_sh ("Paint area defined by shading pattern") has a docstring and no body, so a shading painted with sh reaches no device and pdfplumber 0.11.9 reports nothing for it. On govdocs1 003694 p1 an axial shading (/ShadingType 2) clipped to 297 x 758 pt is most of the page's ink; a pdfplumber map covers 36.55% of it. PyMuPDF's get_bboxlog lists it as fill-shade, but with an unbounded box (plus or minus 2^31), not the clip. Found by the baseline ink run (results/ink-baseline-readers.md).

## PyMuPDF boxes a tiling-pattern fill by its tile, not the area it paints (1 October 2026)

On govdocs1 003404 p0 (/Rotate 90) the page background is a 509 x 792 pt rectangle filled with a coloured tiling pattern (/PatternType 1, /PaintType 1, inside a /Background artifact and an optional content group). MuPDF renders it, about half the page's ink at 150 dpi, but neither get_drawings nor get_bboxlog in PyMuPDF 1.24.9 (MuPDF 1.24.8) gives any box over it. Checked on a one-path file: "/Pattern cs /P0 scn 50 50 100 100 re f" with a 10 pt tile holding one 5 x 5 pt black square. The render has 2,500 dark pixels at 72 dpi, spread over the 100 x 100 area; get_drawings returns one black fill at (0, 195, 5, 200) and get_bboxlog one fill-path at the same box, which is the tile's own content in pattern space at the origin, drawn once. The same rectangle filled with "0 g" is reported at (50, 50, 150, 150). Found by the baseline ink run (tools/ink_baseline.py pymupdf), where 003404 p0 fell to 51.83% while regions-cli covers it fully.

## PyMuPDF's words leave out glyphs that decode to control codes (1 October 2026)

On govdocs1 003437 (fonts AdvPS6F00 and AdvPS6F0B, codes below 32 with no Unicode mapping that changes them) 3,661 of the 4,578 characters rawdict gives on page 7 are control codes, each with a box and each drawn. get_text("words") in PyMuPDF 1.24.9 keeps none of them: 128 words holding 228 characters. A map built from words misses those glyphs' ink altogether (14.88% coverage on p7, all eight pages of the file under 45%); rawdict's character boxes cover them, and regions-cli covers all eight pages at 100%. Not a bug as such, since words are split on whitespace and control codes count as such, but the words call isn't a safe box source for garbled fonts. Found by the baseline ink run.

## MuPDF lists fills it then doesn't render, where pdfium does (29 September 2026)

On govdocs1 003174 p0 (a 4320 x 3024 pt drawing) 21 filled and stroked paths and on 003413 p19 one black fill (230 x 25 pt) appear in PyMuPDF 1.24.9's get_drawings and get_bboxlog, with nothing later in the paint log touching them, but its render has no ink there. pdfium 149.0.7825.0 (pypdfium2 5.8.0) renders ink at every one of the 21 on 003174 and 1,889 dark pixels in the 003413 fill (both at 300 dpi). 003174 has 208 ExtGStates with overprint mode 1 and overprint on in 139, so overprint handling is the likely difference, but that isn't verified. No upstream draft until the cause is found. Found by the ink test's phantom count.

## MuPDF and pdfium draw a stroked point differently, and neither quite as the spec says (29 September 2026)

ISO 32000-1, 8.5.3.2 (page 144 of Adobe's PDF32000_2008.pdf): a degenerate subpath, "a single-point closed path or ... two or more points at the same coordinates", is stroked only with round caps, "producing a filled circle centered at the single point"; with butt or square caps "S shall produce no output". A lone trailing m produces nothing. 8.5.3.3 adds that a fill of a degenerate subpath paints the single device pixel under the point.

Found on govdocs1 003828 p1, which has 1,415 paths of the form "x y m h B*" with line width 0.48 and round caps (1 J). Seven of them, where nothing else is drawn, showed up as phantom paths in the ink test: our box was a bare point, and PyMuPDF's render had no ink there. Checked on a one-path test page (4 w, black, rendered at 288 dpi for MuPDF and scale 4 for pdfium; dark pixels, below 250):

| Content | Spec | MuPDF 1.24.9 (PyMuPDF 1.24.9) | pdfium 149.0.7825.0 (pypdfium2 5.8.0) |
|---|---|---|---|
| 1 J 25 25 m h B* | dot | 0 | 236 |
| 1 J 25 25 m h S | dot | 0 | 236 |
| 1 J 25 25 m 25 25 l S | dot | 216 | 236 |
| 1 J 25 25 m S | nothing | 0 | 0 |
| 0 J 25 25 m h S | nothing | 0 | 0 |
| 2 J 25 25 m 25 25 l S | nothing | 0 | 272 |
| 0 J 25 25 m 25 25 l S | nothing | 0 | 16 |

So MuPDF skips the round dot for a closed single point (m h) but draws it for a zero-length line, and pdfium draws square-capped and butt-capped zero-length lines, which the spec says produce nothing. PyMuPDF's get_drawings reports no drawing for the m h dots either.

This repo follows the spec: such a path's box is the dot (half the line width round the point), flagged dot; a point that is neither a dot nor filled is flagged empty (regions/src/vector.rs stroke_box, lib.rs). The ink test counts phantom dots apart, since MuPDF is its renderer.

## Standard-font ascent and descent differ between two libraries (29 September 2026)

The marks set (tools/build_marks.py) needs each standard font's ascender and descender to give text its true box. Two Python libraries disagree with each other and with Adobe's core-14 AFM files, which are the metrics PDFs refer to for the 14 standard fonts. The AFM files checked are the unmodified copies in Apache PDFBox (pdfbox/src/main/resources/org/apache/pdfbox/resources/afm/).

| Font | Core-14 AFM (Ascender / Descender) | pdfminer.six 20251230 | reportlab 5.0.0 |
|---|---|---|---|
| Courier | 629 / -157 | 627 / -194 | 629 / -157 |
| Courier-Bold | 629 / -157 | 627 / -194 | 626 / -142 |
| Courier-BoldOblique | 629 / -157 | | 626 / -142 |
| Times-Bold | 683 / -217 | 683 / -217 | 676 / -205 |

pdfminer.six (pdfminer/fontmetrics.py) gives its source as "Adobe Core 35 AFM Files", and its Courier FontBBox (-6 -249 639 803) and CapHeight (572) differ from core-14 Courier.afm (-23 -250 715 805, 562). That points to a different Courier release rather than a slip, though the Core 35 files weren't checked here.

reportlab (reportlab/pdfbase/_fontdata.py, ascent_descent) gives Times-Bold 676, which is Times-Bold's CapHeight in the AFM, and Courier-Bold 626 / -142, which matches nothing in Courier-Bold.afm.

This repo now takes the family values straight from the AFM files (regions/src/font.rs, std_vertical). The marks set uses reportlab only for Helvetica, Times-Roman and Courier, whose values match the AFM files.

## Real PDFs write /Descent as a positive number (28 September 2026)

The spec gives /Descent as negative. ContractNLI and govdocs1 files carry "/Descent 270" for Arial Unicode MS and "/Descent 206" for Tahoma. A parser that clamps the value to zero puts every glyph box's bottom on the baseline. Found by the ink test (descenders and underscores outside word boxes, constructed case c0148); fixed by taking the size and forcing the sign.

## PyMuPDF gives boxes on the unrotated page but renders it turned (28 September 2026)

For a page with /Rotate 90, get_text, get_image_info and get_drawings return boxes in the unrotated page's coordinates, while get_pixmap renders the page as displayed. Comparing the two needs page.rotation_matrix. This is documented behaviour, not a bug, but it made the ink test put missed ink down to the wrong kind on rotated pages (govdocs1 003404, 003053) until the boxes were mapped.

## PyMuPDF gives spaces character boxes (28 September 2026)

get_text("rawdict") lists spaces as characters with boxes. Treating those boxes as text counted a page's background, showing between words, as text (govdocs1 003704, a dark blue cover). Also behaviour to know rather than a bug.
