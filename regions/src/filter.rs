//! regions: from a page's images and words to OCR regions (option A: structure only, no pixels).
//!
//! Every image the page draws is either dropped (it can't hold readable text, or the file's own
//! text already explains it) or kept as a region. Option A can't tell a photo from a scanned
//! paragraph, so every image that survives is an "ocr" candidate; any text in an image counts (D67).
//! Rules run in a fixed order and the first that applies names the outcome:
//!
//!   offpage     nothing of it is on the visible page
//!   thin        a rule or a bar: its short side is under MIN_SIDE_PX pixels or MIN_SIDE_PT points,
//!               or it's longer than MAX_ASPECT times its short side
//!   tiny        under MIN_AREA_PT square points: at most a glyph or two
//!   low_dpi     under MIN_DPI pixels per inch along its coarser side: too coarse to read
//!   text_layer  an OCR layer lies on it: at least LAYER_WORDS invisible words centred on it, or
//!               invisible words covering LAYER_COVER of it; already read, reported so it isn't OCR'd again
//!   under_text  visible words lie on it: they cover TEXT_COVER of it, or at least TEXT_WORDS of them
//!               are centred on it and they're at least TEXT_SHARE of the page's visible words
//!               (a background or watermark under the page's text)
//!   ocr         everything else
//!
//! Stencil masks (/ImageMask) are kept and flagged: they paint one colour through a 1-bit shape,
//! which is how some generators draw scanned text and signatures, so dropping them would lose text.

use crate::{Image, Word};

/// A short side under 8 pixels can't hold a line of text: OCR needs about 10 pixels of x-height.
pub const MIN_SIDE_PX: u32 = 8;
/// Under 6 points (about a 4.5 pt font's full height) a line of text isn't readable on the page either.
pub const MIN_SIDE_PT: f64 = 6.0;
/// Longer than 60 times its short side is a rule; a single wide text line stays well under this
/// (a 10 pt line across a 540 pt text column is 54 to 1).
pub const MAX_ASPECT: f64 = 60.0;
/// 150 square points is a 12 x 12.5 pt box: a bullet or an icon, not text worth a region.
pub const MIN_AREA_PT: f64 = 150.0;
/// Under 50 dpi body text is 5 or 6 pixels high and nothing reads it. The plan said about 70, but
/// on govdocs1 003 Tesseract read axis labels on charts drawn at 59 to 68 dpi (5 of 6 text images
/// under 70 were at 50 or more), so the floor is lower; it costs 6 photo-like images flagged there.
pub const MIN_DPI: f64 = 50.0;
/// Share of the image under invisible words that makes it an already-OCR'd image. Word boxes cover
/// a fifth to a half of a text block, so a tenth is a real layer, not a stray word.
pub const LAYER_COVER: f64 = 0.10;
/// Invisible words centred on the image that make an OCR layer even when they cover little of it
/// (a sparse scan, OCR'd at a coarse word size). Nothing but OCR draws invisible words over an image;
/// five keeps a stray hidden word or two from counting.
pub const LAYER_WORDS: usize = 5;
/// Share of the image under visible words that makes it a background. A dense text page's words
/// cover 15 to 40% of it; a caption or two over a picture covers far less.
pub const TEXT_COVER: f64 = 0.10;
/// Area alone misses backgrounds under sparse pages (the tune set has one at 5%), so a background is
/// also an image with at least TEXT_WORDS visible words centred on it, when those are at least
/// TEXT_SHARE of the page's visible words. A figure with digital labels on a page of body text, or a
/// scan with a Bates number stamped on it, doesn't pass both.
pub const TEXT_WORDS: usize = 20;
pub const TEXT_SHARE: f64 = 0.5;
/// A region whose short side is under 36 points (half an inch) is flagged "small".
pub const SMALL_SIDE_PT: f64 = 36.0;

/// One image's outcome: a region to OCR ("ocr"), one already read ("text_layer"), or a drop reason.
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
    pub kind: &'static str,
    pub mask: bool,
    pub annot: bool,
    pub small: bool,
}

impl Region {
    /// A region to OCR or already read; the rest are dropped.
    pub fn kept(&self) -> bool { self.kind == "ocr" || self.kind == "text_layer" }
}

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

/// Every image on the page with its outcome, in the page's image order.
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
        let kind = if w <= 0.0 || h <= 0.0 {
            "offpage"
        } else if m.px_w.min(m.px_h) < MIN_SIDE_PX || short_pt < MIN_SIDE_PT || long_pt > MAX_ASPECT * short_pt {
            "thin"
        } else if w * h < MIN_AREA_PT {
            "tiny"
        } else if dpi < MIN_DPI {
            "low_dpi"
        } else if layer_cover >= LAYER_COVER || layer_words >= LAYER_WORDS {
            "text_layer"
        } else if text_cover >= TEXT_COVER || (text_words >= TEXT_WORDS && text_words as f64 >= TEXT_SHARE * visible as f64) {
            "under_text"
        } else {
            "ocr"
        };
        Region {
            x0: m.x0, y0: m.y0, x1: m.x1, y1: m.y1, image: i, obj: m.obj, dpi,
            share: (w.max(0.0) * h.max(0.0)) / page, text_cover, layer_cover, text_words, layer_words, kind,
            mask: m.mask, annot: m.annot, small: short_pt < SMALL_SIDE_PT,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(x0: f64, y0: f64, x1: f64, y1: f64, dpi: f64) -> Image {
        let px = |pt: f64| (pt / 72.0 * dpi).round() as u32;
        Image { x0, y0, x1, y1, px_w: px(x1 - x0), px_h: px(y1 - y0), dpi_x: dpi, dpi_y: dpi,
            mask: false, inline: false, annot: false, obj: 7, parts: 1, clipped: false, upright: true }
    }

    fn word(x0: f64, y0: f64, x1: f64, y1: f64, invisible: bool) -> Word {
        Word { text: "w".into(), x0, y0, x1, y1, line: 0, font: 0, size: 10.0, unmapped: 0,
            invisible, annot: false, offpage: false, first: 0, count: 1 }
    }

    fn kind(m: Image, words: &[Word]) -> &'static str { regions(&[m], words, 600.0, 800.0)[0].kind }

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
    fn plain_image_is_ocr() {
        let r = &regions(&[img(100.0, 100.0, 400.0, 300.0, 200.0)], &[], 600.0, 800.0)[0];
        assert_eq!(r.kind, "ocr");
        assert!((r.share - 300.0 * 200.0 / (600.0 * 800.0)).abs() < 1e-9);
        assert!((r.dpi - 200.0).abs() < 1e-9 && r.obj == 7 && r.image == 0 && !r.small);
    }

    #[test]
    fn thin_rules_are_dropped() {
        // 3 pt high: under MIN_SIDE_PT
        assert_eq!(kind(img(50.0, 50.0, 500.0, 53.0, 300.0), &[]), "thin");
        // 10 pt high but only 5 pixels
        assert_eq!(kind(img(50.0, 50.0, 500.0, 60.0, 36.0), &[]), "thin");
        // 7 pt by 500 pt: over MAX_ASPECT
        assert_eq!(kind(img(50.0, 50.0, 550.0, 57.0, 300.0), &[]), "thin");
        // a single text line, 12 pt by 500 pt, stays
        assert_eq!(kind(img(50.0, 50.0, 550.0, 62.0, 300.0), &[]), "ocr");
    }

    #[test]
    fn tiny_images_are_dropped() {
        assert_eq!(kind(img(50.0, 50.0, 60.0, 60.0, 300.0), &[]), "tiny");
        assert_eq!(kind(img(50.0, 50.0, 70.0, 60.0, 300.0), &[]), "ocr");
    }

    #[test]
    fn coarse_images_are_dropped() {
        assert_eq!(kind(img(50.0, 50.0, 300.0, 300.0, 40.0), &[]), "low_dpi");
        assert_eq!(kind(img(50.0, 50.0, 300.0, 300.0, 60.0), &[]), "ocr");
        // the coarser side decides
        let mut m = img(50.0, 50.0, 300.0, 300.0, 300.0);
        m.dpi_y = 45.0;
        assert_eq!(kind(m, &[]), "low_dpi");
    }

    #[test]
    fn nothing_on_the_page_is_offpage() {
        assert_eq!(kind(img(50.0, 50.0, 50.0, 300.0, 300.0), &[]), "offpage");
    }

    #[test]
    fn background_under_text_is_dropped() {
        let words = block(0.0, 0.0, 600.0, 800.0, false);
        assert_eq!(kind(img(0.0, 0.0, 600.0, 800.0, 150.0), &words), "under_text");
    }

    #[test]
    fn background_under_a_sparse_page_is_dropped() {
        // 30 words, all on the image, covering under 1% of it
        let words: Vec<Word> = (0..30).map(|i| word(50.0, 20.0 * i as f64, 60.0, 20.0 * i as f64 + 5.0, false)).collect();
        let r = &regions(&[img(0.0, 0.0, 600.0, 800.0, 150.0)], &words, 600.0, 800.0)[0];
        assert!(r.text_cover < TEXT_COVER && r.text_words == 30);
        assert_eq!(r.kind, "under_text");
    }

    #[test]
    fn a_labelled_figure_on_a_text_page_stays() {
        // 25 labels on a figure, 100 words of body text elsewhere: the labels are a fifth of the page's words
        let mut words: Vec<Word> = (0..25).map(|i| word(110.0, 100.0 + 8.0 * i as f64, 118.0, 104.0 + 8.0 * i as f64, false)).collect();
        words.extend((0..100).map(|i| word(50.0, 400.0 + 4.0 * i as f64, 58.0, 403.0 + 4.0 * i as f64, false)));
        assert_eq!(kind(img(100.0, 100.0, 400.0, 300.0, 200.0), &words), "ocr");
    }

    #[test]
    fn a_stamp_on_a_scan_keeps_it() {
        let words: Vec<Word> = (0..3).map(|i| word(500.0 + 20.0 * i as f64, 780.0, 515.0 + 20.0 * i as f64, 790.0, false)).collect();
        assert_eq!(kind(img(0.0, 0.0, 600.0, 800.0, 300.0), &words), "ocr");
    }

    #[test]
    fn a_sparse_layer_is_text_layer() {
        let words: Vec<Word> = (0..6).map(|i| word(110.0, 110.0 + 20.0 * i as f64, 120.0, 115.0 + 20.0 * i as f64, true)).collect();
        let r = &regions(&[img(100.0, 100.0, 400.0, 300.0, 300.0)], &words, 600.0, 800.0)[0];
        assert!(r.layer_cover < LAYER_COVER && r.kind == "text_layer");
        // four hidden words aren't a layer
        assert_eq!(kind(img(100.0, 100.0, 400.0, 300.0, 300.0), &words[..4]), "ocr");
    }

    #[test]
    fn a_caption_over_a_picture_keeps_it() {
        // one line of words across the bottom of a 300 x 200 picture: 5% of it
        let words = block(100.0, 290.0, 400.0, 300.0, false);
        let r = &regions(&[img(100.0, 100.0, 400.0, 300.0, 200.0)], &words, 600.0, 800.0)[0];
        assert!(r.text_cover > 0.0 && r.text_cover < TEXT_COVER);
        assert_eq!(r.kind, "ocr");
    }

    #[test]
    fn invisible_layer_is_text_layer() {
        let words = block(100.0, 100.0, 400.0, 300.0, true);
        assert_eq!(kind(img(100.0, 100.0, 400.0, 300.0, 300.0), &words), "text_layer");
        // an OCR'd full scan: invisible words everywhere and no visible ones
        let words = block(0.0, 0.0, 600.0, 800.0, true);
        assert_eq!(kind(img(0.0, 0.0, 600.0, 800.0, 300.0), &words), "text_layer");
    }

    #[test]
    fn text_elsewhere_on_the_page_doesnt_count() {
        let words = block(0.0, 400.0, 600.0, 800.0, false);
        assert_eq!(kind(img(100.0, 100.0, 400.0, 300.0, 200.0), &words), "ocr");
    }

    #[test]
    fn offpage_words_dont_count() {
        let mut words = block(100.0, 100.0, 400.0, 300.0, false);
        for w in &mut words { w.offpage = true; }
        assert_eq!(kind(img(100.0, 100.0, 400.0, 300.0, 200.0), &words), "ocr");
    }

    #[test]
    fn masks_are_kept_and_flagged() {
        let mut m = img(100.0, 100.0, 400.0, 300.0, 300.0);
        m.mask = true;
        let r = &regions(&[m], &[], 600.0, 800.0)[0];
        assert!(r.kind == "ocr" && r.mask);
    }

    #[test]
    fn small_flag() {
        let r = &regions(&[img(100.0, 100.0, 400.0, 130.0, 300.0)], &[], 600.0, 800.0)[0];
        assert!(r.kind == "ocr" && r.small);
    }
}
