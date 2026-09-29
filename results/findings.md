# Findings about other software and real files

Facts the checks turned up about libraries, data sources and PDFs in the wild, as opposed to bugs in this repo's own code. Each entry says how it was found and what it was checked against.

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
