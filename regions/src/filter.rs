//! regions: from a page's images and words to OCR regions (option A: structure only, no pixels).
//!
//! Every image the page draws becomes a region; none is dropped (D69). Each carries a confidence,
//! 0 to 1, that it holds text the file doesn't already give as characters, and the reasons that
//! lowered it. The caller picks the cutoff: a strict pipeline OCRs nearly everything, a cheap one
//! only the top. Option A can't tell a photo from a scanned paragraph, so a plain image starts at
//! BASE, not 1; option B's pixel test is meant to move it. Any text in an image counts (D67).
//!
//! Each piece of evidence multiplies the confidence by a factor between its floor and 1:
//!
//!   offpage     nothing of it is on the visible page (on the pasteboard, cropped or clipped away)
//!   narrow_px   its short side has few pixels: OCR needs about 10 pixels of x-height
//!   narrow      its short side is only a few points: too small to read on the page too
//!   long        it's many times longer than its short side, as a rule or a bar is
//!   tiny        its area is a bullet or an icon's
//!   low_dpi     its coarser side has few pixels per inch
//!   text_layer  invisible words lie on it: an OCR layer, so it's already been read
//!   under_text  visible words lie on it: a background or watermark under the page's text, or a
//!               scan with a visible text layer (drawing order isn't tracked, so it's never ruled out)
//!
//! Each factor ramps between two limits, and the old hard limit (D68) sits in the middle of the
//! ramp, where a region with nothing else against it lands just above CUT.
//!
//! Stencil masks (/ImageMask) are flagged, not scored down: they paint one colour through a 1-bit
//! shape, which is how some generators draw scanned text and signatures.

use crate::{Image, Word};

/// Where a plain image starts: structure alone can't say it holds text, only that it could.
pub const BASE: f64 = 0.8;
/// A suggested cutoff: at or above it, OCR the region. Tools score against it; callers choose.
pub const CUT: f64 = 0.4;

/// An image outside the visible page: nobody sees its text, but it's still reported.
pub const OFFPAGE: f64 = 0.02;
/// Short side in pixels: 4 or fewer can't hold a line, 12 or more can (old limit 8).
pub const SIDE_PX: (f64, f64, f64) = (4.0, 12.0, 0.05);
/// Short side in points: 3 or less is a hairline, 9 or more fits a line of small text (old limit 6).
pub const SIDE_PT: (f64, f64, f64) = (3.0, 9.0, 0.05);
/// Length over short side: 80 or more is a rule, 40 or less isn't (old limit 60). A single wide text
/// line stays under the middle: a 10 pt line across a 540 pt column is 54 to 1.
pub const ASPECT: (f64, f64, f64) = (80.0, 40.0, 0.05);
/// Square points: 50 or less is a glyph or two, 250 or more can hold a word (old limit 150).
pub const AREA_PT: (f64, f64, f64) = (50.0, 250.0, 0.05);
/// Dots per inch along the coarser side: under 30 nothing reads it, from 70 body text is fine
/// (old limit 50; on govdocs1 003 Tesseract read chart axis labels drawn at 59 to 68 dpi).
pub const DPI: (f64, f64, f64) = (30.0, 70.0, 0.05);
/// OCR layer: invisible words cover 2% to 10% of the image, or 1 to 5 are centred on it; whichever
/// is stronger. Nothing but OCR draws invisible words over an image. Already read, so the floor is low.
pub const LAYER_COVER: (f64, f64) = (0.02, 0.10);
pub const LAYER_WORDS: (f64, f64) = (1.0, 5.0);
pub const LAYER_FLOOR: f64 = 0.05;
/// Visible words: they cover 3% to 10% of the image (a dense page's words cover 15 to 40%, a caption
/// far less), or 5 to 20 of them are centred on it while they're a quarter to a half of the page's
/// visible words (a background under a sparse page). The floor stays high: 38 text images in 003 are
/// scans with a visible text layer, and this evidence can't tell them from backgrounds.
pub const TEXT_COVER: (f64, f64) = (0.03, 0.10);
pub const TEXT_WORDS: (f64, f64) = (5.0, 20.0);
pub const TEXT_SHARE: (f64, f64) = (0.25, 0.5);
pub const TEXT_FLOOR: f64 = 0.25;
/// A region whose short side is under 36 points (half an inch) is flagged "small".
pub const SMALL_SIDE_PT: f64 = 36.0;

/// One image as a region: its box, the evidence, its confidence and what lowered it.
#[derive(Clone, Debug)]
pub struct Region {
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    /// Index into `Page::images`, and that image's object number (0 for inline images).
    pub image: usize,
    pub obj: u32,
    /// Effective resolution: the coarser of the image's two.
    pub dpi: f64,
    /// Share of the page's area.
    pub share: f64,
    /// Share of the image under visible and under invisible words, and how many of each are centred on it.
    pub text_cover: f64,
    pub layer_cover: f64,
    pub text_words: usize,
    pub layer_words: usize,
    /// That it holds text not yet read, 0 to 1.
    pub confidence: f64,
    /// Each piece of evidence against it and the factor it applied, in the order above.
    pub reasons: Vec<(&'static str, f64)>,
    pub mask: bool,
    pub annot: bool,
    pub small: bool,
}

/// Where x falls between lo and hi, 0 to 1 (hi may be below lo for a falling ramp).
fn ramp(x: f64, lo: f64, hi: f64) -> f64 { ((x - lo) / (hi - lo)).clamp(0.0, 1.0) }

/// A factor from its floor (s = 0) to 1 (s = 1).
fn factor(s: f64, floor: f64) -> f64 { floor + (1.0 - floor) * s }

/// Share of the box (x0, y0, x1, y1) covered by the words, each word clipped to it. Words rarely
/// overlap each other, so the sum stands in for the union; it's capped at 1.
fn cover<'a>(b: (f64, f64, f64, f64), words: impl Iterator<Item = &'a Word>) -> f64 {
    let area = (b.2 - b.0) * (b.3 - b.1);
    if area <= 0.0 { return 0.0; }
    let s: f64 = words.map(|w| {
        let dx = w.x1.min(b.2) - w.x0.max(b.0);
        let dy = w.y1.min(b.3) - w.y0.max(b.1);
        if dx > 0.0 && dy > 0.0 { dx * dy } else { 0.0 }
    }).sum();
    (s / area).min(1.0)
}

/// How many of the words have their centre inside the box.
fn centred<'a>(b: (f64, f64, f64, f64), words: impl Iterator<Item = &'a Word>) -> usize {
    words.filter(|w| {
        let (x, y) = ((w.x0 + w.x1) / 2.0, (w.y0 + w.y1) / 2.0);
        x >= b.0 && x <= b.2 && y >= b.1 && y <= b.3
    }).count()
}

/// Every image on the page as a region, in the page's image order.
pub fn regions(images: &[Image], words: &[Word], width: f64, height: f64) -> Vec<Region> {
    let page = (width * height).max(1e-9);
    images.iter().enumerate().map(|(i, m)| {
        let (w, h) = (m.x1 - m.x0, m.y1 - m.y0);
        let b = (m.x0, m.y0, m.x1, m.y1);
        let dpi = m.dpi_x.min(m.dpi_y);
        let shown = || words.iter().filter(|w| !w.offpage);
        let text_cover = cover(b, shown().filter(|w| !w.invisible));
        let layer_cover = cover(b, shown().filter(|w| w.invisible));
        let text_words = centred(b, shown().filter(|w| !w.invisible));
        let layer_words = centred(b, shown().filter(|w| w.invisible));
        let visible = shown().filter(|w| !w.invisible).count();
        let (short_pt, long_pt) = (w.min(h), w.max(h));

        let mut reasons: Vec<(&'static str, f64)> = Vec::new();
        if m.offpage || w <= 0.0 || h <= 0.0 {
            // nothing visible: the size rules have nothing to measure
            reasons.push(("offpage", OFFPAGE));
        } else {
            let mut add = |why, f: f64| if f < 1.0 { reasons.push((why, f)) };
            let short_px = m.px_w.min(m.px_h) as f64;
            add("narrow_px", factor(ramp(short_px, SIDE_PX.0, SIDE_PX.1), SIDE_PX.2));
            add("narrow", factor(ramp(short_pt, SIDE_PT.0, SIDE_PT.1), SIDE_PT.2));
            add("long", factor(ramp(long_pt / short_pt, ASPECT.0, ASPECT.1), ASPECT.2));
            add("tiny", factor(ramp(w * h, AREA_PT.0, AREA_PT.1), AREA_PT.2));
            add("low_dpi", factor(ramp(dpi, DPI.0, DPI.1), DPI.2));
            let layer = ramp(layer_cover, LAYER_COVER.0, LAYER_COVER.1)
                .max(ramp(layer_words as f64, LAYER_WORDS.0, LAYER_WORDS.1));
            add("text_layer", factor(1.0 - layer, LAYER_FLOOR));
            let share = if visible > 0 { text_words as f64 / visible as f64 } else { 0.0 };
            let text = ramp(text_cover, TEXT_COVER.0, TEXT_COVER.1)
                .max(ramp(text_words as f64, TEXT_WORDS.0, TEXT_WORDS.1) * ramp(share, TEXT_SHARE.0, TEXT_SHARE.1));
            add("under_text", factor(1.0 - text, TEXT_FLOOR));
        }
        let confidence = reasons.iter().fold(BASE, |c, r| c * r.1);
        Region {
            x0: m.x0, y0: m.y0, x1: m.x1, y1: m.y1, image: i, obj: m.obj, dpi,
            share: (w.max(0.0) * h.max(0.0)) / page, text_cover, layer_cover, text_words, layer_words,
            confidence, reasons, mask: m.mask, annot: m.annot, small: short_pt < SMALL_SIDE_PT,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(x0: f64, y0: f64, x1: f64, y1: f64, dpi: f64) -> Image {
        let px = |pt: f64| (pt / 72.0 * dpi).round() as u32;
        Image { x0, y0, x1, y1, px_w: px(x1 - x0), px_h: px(y1 - y0), dpi_x: dpi, dpi_y: dpi,
            mask: false, inline: false, annot: false, obj: 7, parts: 1, clipped: false, upright: true, offpage: false, order: 0 }
    }

    fn word(x0: f64, y0: f64, x1: f64, y1: f64, invisible: bool) -> Word {
        Word { text: "w".into(), x0, y0, x1, y1, line: 0, font: 0, size: 10.0, unmapped: 0,
            invisible, annot: false, offpage: false, first: 0, count: 1, order: 0 }
    }

    fn one(m: Image, words: &[Word]) -> Region { regions(&[m], words, 600.0, 800.0).remove(0) }

    fn why(r: &Region) -> Vec<&'static str> { r.reasons.iter().map(|r| r.0).collect() }

    /// A grid of word boxes over (x0, y0)..(x1, y1): 40 x 10 pt words on 14 pt lines, 5 pt apart,
    /// covering about half of the area, as a text block does.
    fn block(x0: f64, y0: f64, x1: f64, y1: f64, invisible: bool) -> Vec<Word> {
        let mut v = Vec::new();
        let mut y = y0;
        while y + 10.0 <= y1 {
            let mut x = x0;
            while x + 40.0 <= x1 { v.push(word(x, y, x + 40.0, y + 10.0, invisible)); x += 45.0; }
            y += 14.0;
        }
        v
    }

    #[test]
    fn plain_image_starts_at_base() {
        let r = one(img(100.0, 100.0, 400.0, 300.0, 200.0), &[]);
        assert!((r.confidence - BASE).abs() < 1e-9 && r.reasons.is_empty());
        assert!((r.share - 300.0 * 200.0 / (600.0 * 800.0)).abs() < 1e-9);
        assert!((r.dpi - 200.0).abs() < 1e-9 && r.obj == 7 && r.image == 0 && !r.small);
    }

    #[test]
    fn every_image_is_a_region() {
        let ms = [img(50.0, 50.0, 500.0, 51.0, 300.0), img(50.0, 50.0, 50.0, 300.0, 300.0), img(10.0, 10.0, 12.0, 12.0, 20.0)];
        let rs = regions(&ms, &[], 600.0, 800.0);
        assert_eq!(rs.len(), 3);
        assert!(rs.iter().all(|r| r.confidence > 0.0 && r.confidence < CUT));
    }

    #[test]
    fn thin_rules_score_low() {
        // 3 pt high: at the floor of "narrow"
        let r = one(img(50.0, 50.0, 500.0, 53.0, 300.0), &[]);
        assert!(r.confidence < 0.1 && why(&r).contains(&"narrow"));
        // 10 pt high but only 5 pixels
        let r = one(img(50.0, 50.0, 500.0, 60.0, 36.0), &[]);
        assert!(r.confidence < CUT && why(&r).contains(&"narrow_px"));
        // 7 pt by 500 pt: long
        let r = one(img(50.0, 50.0, 550.0, 57.0, 300.0), &[]);
        assert!(r.confidence < CUT && why(&r).contains(&"long"));
        // a single text line, 12 pt by 500 pt, stays over the cut
        assert!(one(img(50.0, 50.0, 550.0, 62.0, 300.0), &[]).confidence >= CUT);
    }

    #[test]
    fn tiny_images_score_low() {
        let r = one(img(50.0, 50.0, 60.0, 60.0, 300.0), &[]);
        assert!(r.confidence < CUT && why(&r).contains(&"tiny"));
        assert!(one(img(50.0, 50.0, 70.0, 60.0, 300.0), &[]).confidence >= CUT);
    }

    #[test]
    fn coarse_images_score_lower_gradually() {
        let at = |dpi| one(img(50.0, 50.0, 300.0, 300.0, dpi), &[]).confidence;
        assert!(at(25.0) < 0.1 && at(40.0) < CUT);
        assert!(at(40.0) < at(50.0) && at(50.0) < at(60.0) && at(60.0) < at(70.0));
        // the old hard limit sits just above the cut; chart labels at 59 to 68 dpi are well over it
        assert!(at(50.0) >= CUT && at(59.0) > 0.5);
        assert!((at(70.0) - BASE).abs() < 1e-9);
        // the coarser side decides
        let mut m = img(50.0, 50.0, 300.0, 300.0, 300.0);
        m.dpi_y = 35.0;
        assert!(one(m, &[]).confidence < CUT);
    }

    #[test]
    fn nothing_on_the_page_is_offpage() {
        let r = one(img(50.0, 50.0, 50.0, 300.0, 300.0), &[]);
        assert_eq!(why(&r), ["offpage"]);
        assert!((r.confidence - BASE * OFFPAGE).abs() < 1e-9);
    }

    #[test]
    fn an_image_placed_off_the_page_is_offpage() {
        let mut m = img(700.0, 50.0, 900.0, 300.0, 300.0);
        m.offpage = true;
        let r = one(m, &[]);
        assert_eq!(why(&r), ["offpage"]);
    }

    #[test]
    fn background_under_text_scores_low_but_not_zero() {
        let words = block(0.0, 0.0, 600.0, 800.0, false);
        let r = one(img(0.0, 0.0, 600.0, 800.0, 150.0), &words);
        assert_eq!(why(&r), ["under_text"]);
        assert!((r.confidence - BASE * TEXT_FLOOR).abs() < 1e-9);
    }

    #[test]
    fn background_under_a_sparse_page_scores_low() {
        // 30 words, all on the image, covering under 1% of it
        let words: Vec<Word> = (0..30).map(|i| word(50.0, 20.0 * i as f64, 60.0, 20.0 * i as f64 + 5.0, false)).collect();
        let r = one(img(0.0, 0.0, 600.0, 800.0, 150.0), &words);
        assert!(r.text_cover < TEXT_COVER.0 && r.text_words == 30);
        assert!(r.confidence < CUT && why(&r) == ["under_text"]);
    }

    #[test]
    fn a_labelled_figure_on_a_text_page_stays_high() {
        // 25 labels on a figure, 100 words of body text elsewhere: the labels are a fifth of the page's words
        let mut words: Vec<Word> = (0..25).map(|i| word(110.0, 100.0 + 8.0 * i as f64, 118.0, 104.0 + 8.0 * i as f64, false)).collect();
        words.extend((0..100).map(|i| word(50.0, 400.0 + 4.0 * i as f64, 58.0, 403.0 + 4.0 * i as f64, false)));
        assert!(one(img(100.0, 100.0, 400.0, 300.0, 200.0), &words).confidence >= CUT);
    }

    #[test]
    fn a_stamp_on_a_scan_keeps_it_high() {
        let words: Vec<Word> = (0..3).map(|i| word(500.0 + 20.0 * i as f64, 780.0, 515.0 + 20.0 * i as f64, 790.0, false)).collect();
        assert!(one(img(0.0, 0.0, 600.0, 800.0, 300.0), &words).confidence >= CUT);
    }

    #[test]
    fn a_sparse_layer_is_text_layer() {
        let words: Vec<Word> = (0..6).map(|i| word(110.0, 110.0 + 20.0 * i as f64, 120.0, 115.0 + 20.0 * i as f64, true)).collect();
        let r = one(img(100.0, 100.0, 400.0, 300.0, 300.0), &words);
        assert!(r.layer_cover < LAYER_COVER.1 && why(&r) == ["text_layer"]);
        assert!((r.confidence - BASE * LAYER_FLOOR).abs() < 1e-9);
        // one hidden word isn't a layer
        assert!(one(img(100.0, 100.0, 400.0, 300.0, 300.0), &words[..1]).confidence >= CUT);
    }

    #[test]
    fn a_caption_over_a_picture_keeps_it_high() {
        // one line of words across the bottom of a 300 x 200 picture: 5% of it
        let words = block(100.0, 290.0, 400.0, 300.0, false);
        let r = one(img(100.0, 100.0, 400.0, 300.0, 200.0), &words);
        assert!(r.text_cover > 0.0 && r.text_cover < TEXT_COVER.1);
        assert!(r.confidence >= CUT);
    }

    #[test]
    fn invisible_layer_is_text_layer() {
        let words = block(100.0, 100.0, 400.0, 300.0, true);
        assert!(why(&one(img(100.0, 100.0, 400.0, 300.0, 300.0), &words)) == ["text_layer"]);
        // an OCR'd full scan: invisible words everywhere and no visible ones
        let words = block(0.0, 0.0, 600.0, 800.0, true);
        let r = one(img(0.0, 0.0, 600.0, 800.0, 300.0), &words);
        assert!(why(&r) == ["text_layer"] && r.confidence < CUT);
    }

    #[test]
    fn text_elsewhere_on_the_page_doesnt_count() {
        let words = block(0.0, 400.0, 600.0, 800.0, false);
        let r = one(img(100.0, 100.0, 400.0, 300.0, 200.0), &words);
        assert!(r.reasons.is_empty() && (r.confidence - BASE).abs() < 1e-9);
    }

    #[test]
    fn offpage_words_dont_count() {
        let mut words = block(100.0, 100.0, 400.0, 300.0, false);
        for w in &mut words { w.offpage = true; }
        assert!(one(img(100.0, 100.0, 400.0, 300.0, 200.0), &words).reasons.is_empty());
    }

    #[test]
    fn reasons_multiply() {
        // coarse and small at once scores below either alone
        let both = one(img(50.0, 50.0, 64.0, 62.0, 45.0), &[]);
        assert!(both.reasons.len() >= 2);
        let product: f64 = both.reasons.iter().map(|r| r.1).product();
        assert!((both.confidence - BASE * product).abs() < 1e-9);
    }

    #[test]
    fn masks_are_flagged_not_scored_down() {
        let mut m = img(100.0, 100.0, 400.0, 300.0, 300.0);
        m.mask = true;
        let r = one(m, &[]);
        assert!(r.mask && (r.confidence - BASE).abs() < 1e-9);
    }

    #[test]
    fn small_flag() {
        let r = one(img(100.0, 100.0, 400.0, 130.0, 300.0), &[]);
        assert!(r.confidence >= CUT && r.small);
    }
}
