//! The page map (D70, plans/page-map.md): every region the page draws, in drawing order, each with
//! its box, what drew it and flags. This is the exact layer: nothing here is a guess about what a
//! region looks like. Text is mapped by line (words are in `Page::words`), vector paths by cluster
//! (paths are in `Page::paths`); annotations as their own regions aren't mapped yet.

use crate::{Image, Vector, Word};

/// Words on one line, drawn the same way (all visible or all invisible, all page or all annotation).
#[derive(Clone, Debug)]
pub struct Line {
    pub text: String,
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    /// Index of the line's first word in `Page::words`, and how many words it has.
    pub first: usize, pub count: usize,
    pub order: u32,
    pub glyphs: usize,
    pub unmapped: usize,
    pub invisible: bool, pub annot: bool, pub offpage: bool,
}

/// Words to lines: consecutive words the word grouping put on the same line, split where the way
/// they're drawn changes.
pub fn lines(words: &[Word]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let joins = out.last().is_some_and(|l| {
            let p = &words[l.first + l.count - 1];
            p.line == w.line && p.invisible == w.invisible && p.annot == w.annot
        });
        if joins {
            let l = out.last_mut().unwrap();
            l.text.push(' ');
            l.text.push_str(&w.text);
            l.x0 = l.x0.min(w.x0); l.y0 = l.y0.min(w.y0); l.x1 = l.x1.max(w.x1); l.y1 = l.y1.max(w.y1);
            l.count += 1;
            l.order = l.order.min(w.order);
            l.glyphs += w.count;
            l.unmapped += w.unmapped;
            l.offpage &= w.offpage;
        } else {
            out.push(Line {
                text: w.text.clone(), x0: w.x0, y0: w.y0, x1: w.x1, y1: w.y1, first: i, count: 1, order: w.order,
                glyphs: w.count, unmapped: w.unmapped, invisible: w.invisible, annot: w.annot, offpage: w.offpage,
            });
        }
    }
    out
}

/// One region of the map.
#[derive(Clone, Debug)]
pub struct Entry {
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    /// "text" (a line), "image" or "vector" (touching paths).
    pub what: &'static str,
    /// Index into `Page::lines`, `Page::images` or `Page::vectors`.
    pub index: usize,
    pub order: u32,
    pub flags: Vec<&'static str>,
}

/// Lines, images and vector clusters as one list, in drawing order (a later region is drawn over
/// an earlier one; a cluster's order is its first path's).
pub fn map(lines: &[Line], images: &[Image], vectors: &[Vector]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::with_capacity(lines.len() + images.len() + vectors.len());
    for (i, l) in lines.iter().enumerate() {
        let mut flags = Vec::new();
        if l.invisible { flags.push("invisible"); }
        if l.annot { flags.push("annot"); }
        if l.offpage { flags.push("offpage"); }
        // at least half its glyphs have no usable Unicode: drawn text the file can't give as characters
        if l.unmapped * 2 >= l.glyphs && l.unmapped > 0 { flags.push("undecodable"); }
        out.push(Entry { x0: l.x0, y0: l.y0, x1: l.x1, y1: l.y1, what: "text", index: i, order: l.order, flags });
    }
    for (i, m) in images.iter().enumerate() {
        let mut flags = Vec::new();
        if m.mask { flags.push("mask"); }
        if m.inline { flags.push("inline"); }
        if m.annot { flags.push("annot"); }
        if m.offpage { flags.push("offpage"); } else if m.clipped { flags.push("clipped"); }
        if !m.upright { flags.push("rotated"); }
        if m.parts > 1 { flags.push("strips"); }
        out.push(Entry { x0: m.x0, y0: m.y0, x1: m.x1, y1: m.y1, what: "image", index: i, order: m.order, flags });
    }
    for (i, v) in vectors.iter().enumerate() {
        let mut flags = Vec::new();
        if v.fill { flags.push("fill"); }
        if v.stroke { flags.push("stroke"); }
        if v.shading { flags.push("shading"); }
        if v.white { flags.push("white"); }
        if v.annot { flags.push("annot"); }
        if v.offpage { flags.push("offpage"); }
        out.push(Entry { x0: v.x0, y0: v.y0, x1: v.x1, y1: v.y1, what: "vector", index: i, order: v.order, flags });
    }
    out.sort_by_key(|e| e.order);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x0: f64, line: usize, order: u32, invisible: bool) -> Word {
        Word { text: text.into(), x0, y0: 100.0, x1: x0 + 30.0, y1: 110.0, line, font: 0, size: 10.0, unmapped: 0,
            invisible, annot: false, offpage: false, first: 0, count: 3, order }
    }

    #[test]
    fn words_on_a_line_make_one_region() {
        let ws = [word("one", 10.0, 0, 0, false), word("two", 50.0, 0, 3, false), word("three", 10.0, 1, 6, false)];
        let ls = lines(&ws);
        assert_eq!(ls.len(), 2);
        assert!(ls[0].text == "one two" && ls[0].count == 2 && (ls[0].x1 - 80.0).abs() < 1e-9 && ls[0].glyphs == 6);
        assert_eq!(ls[1].first, 2);
    }

    #[test]
    fn a_change_to_invisible_splits_the_line() {
        let ws = [word("seen", 10.0, 0, 0, false), word("hidden", 50.0, 0, 3, true)];
        let ls = lines(&ws);
        assert!(ls.len() == 2 && ls[1].invisible);
    }

    #[test]
    fn the_map_is_in_drawing_order() {
        let ls = lines(&[word("over", 10.0, 0, 5, false)]);
        let im = Image { x0: 0.0, y0: 0.0, x1: 600.0, y1: 800.0, px_w: 2500, px_h: 3300, dpi_x: 300.0, dpi_y: 300.0,
            mask: false, inline: false, annot: false, obj: 4, parts: 1, clipped: false, upright: true, offpage: false, order: 2 };
        let m = map(&ls, &[im], &[]);
        assert!(m.len() == 2 && m[0].what == "image" && m[1].what == "text");
    }

    #[test]
    fn mostly_undecodable_lines_are_flagged() {
        let mut w = word("\u{fffd}\u{fffd}a", 10.0, 0, 0, false);
        w.unmapped = 2;
        let m = map(&lines(&[w]), &[], &[]);
        assert_eq!(m[0].flags, ["undecodable"]);
    }
}
