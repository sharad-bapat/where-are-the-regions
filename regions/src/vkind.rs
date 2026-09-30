//! The kind layer for vectors (plans/chunk9-kind-vectors.md, 9b): what each vector cluster is, from
//! its paths' boxes, paint and shape counts: a rule, a border, a table grid, a fill, a chart or
//! diagram, or outlined text (letters drawn as filled paths). Each kind has a confidence and the
//! reasons, and outlined text a has-text confidence, as for images (D69, D86, D90). Letters drawn
//! as paths often fall into many small clusters, so outlined text is judged page-wide: glyph-like
//! filled paths in word-like runs (letter_runs, which links letters by vertical overlap).
//! The rules are first cuts; 9c tunes them on tuning data only.

use crate::vector::{Path, Vector};

#[derive(Clone, Debug, Default)]
pub struct VFeatures {
    /// Paths that paint something (not white, empty, hidden or off the page).
    pub paths: u32,
    /// Thin horizontal and vertical lines: at most THIN points across and at least 4 times as long.
    pub hlines: u32, pub vlines: u32,
    /// Distinct rows of the horizontal lines spanning half the cluster's width or more, and columns of
    /// the vertical ones spanning half its height (to a point).
    pub rows: u32, pub cols: u32,
    /// Stroked rectangles (the re operator), and of them the ones that aren't filled.
    pub rects: u32, pub open_rects: u32,
    /// Distinct left and top edges of the stroked rectangles (a grid of cells has several of each),
    /// and the share of the cluster's box they cover (cells tile it; a diagram's boxes don't).
    pub rect_cols: u32, pub rect_rows: u32,
    pub rect_cover: f64,
    /// Filled paths' area over the cluster's box (capped at 1).
    pub filled: f64,
    /// Paths with curves.
    pub curved: u32,
    /// Paths that are glyph-like and in a word-like run on the page.
    pub glyphs: u32,
    /// The cluster's width and height in points.
    pub w: f64, pub h: f64,
}

pub struct VKind {
    pub kind: &'static str,
    pub confidence: f64,
    pub reasons: Vec<(&'static str, f64)>,
    pub has_text: f64,
    pub features: VFeatures,
}

/// A line this thin (points across) or thinner is a rule, if it's also long.
pub const THIN: f64 = 3.0;

fn shows(p: &Path) -> bool { !(p.white || p.empty || p.hidden || p.offpage) }

fn thin_h(p: &Path) -> bool { let (w, h) = (p.x1 - p.x0, p.y1 - p.y0); h <= THIN && w >= 4.0 * h.max(0.25) }
fn thin_v(p: &Path) -> bool { let (w, h) = (p.x1 - p.x0, p.y1 - p.y0); w <= THIN && h >= 4.0 * w.max(0.25) }

/// A filled path shaped like a letter: filled, not a rectangle, 2 to 80 points tall, no more than 3
/// times as wide as tall, with curves or several straight segments.
fn glyph_like(p: &Path) -> bool {
    let (w, h) = (p.x1 - p.x0, p.y1 - p.y0);
    shows(p) && p.fill && !p.stroke && !p.rect && (2.0..=80.0).contains(&h) && w <= 3.0 * h && (p.curves > 0 || p.lines >= 3)
}

/// For each path on the page, whether it's a glyph-like path in a word-like run of 3 or more.
pub fn glyph_runs(paths: &[Path]) -> Vec<bool> {
    let idx: Vec<usize> = (0..paths.len()).filter(|&i| glyph_like(&paths[i])).collect();
    let boxes: Vec<[f64; 4]> = idx.iter().map(|&i| { let p = &paths[i]; [p.x0, p.y0, p.x1, p.y1] }).collect();
    let len = letter_runs(&boxes);
    let mut out = vec![false; paths.len()];
    for (k, &i) in idx.iter().enumerate() { out[i] = len[k] >= 3; }
    out
}

/// Word-like runs of letter boxes (points): each letter is linked to the nearest letter to its right
/// that overlaps it vertically by at least half the smaller height (so a descender doesn't break the
/// word, as matching bottoms would), is 0.4 to 2.5 times as tall, and starts no more than a quarter
/// of its height before its right edge and no more than one height after. Returns each letter's
/// run length (1 when it's in none).
fn letter_runs(b: &[[f64; 4]]) -> Vec<u32> {
    let n = b.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| b[i][0].partial_cmp(&b[j][0]).unwrap().then(i.cmp(&j)));
    let xs: Vec<f64> = order.iter().map(|&i| b[i][0]).collect();
    let mut next = vec![usize::MAX; n];
    let mut has_prev = vec![false; n];
    for i in 0..n {
        let h = b[i][3] - b[i][1];
        let (lo, hi) = (b[i][2] - 0.25 * h, b[i][2] + h);
        let start = xs.partition_point(|&x| x < lo);
        let mut best: Option<(f64, usize)> = None;
        for &j in order[start..].iter().take(400) {
            if b[j][0] > hi { break; }
            if j == i || b[j][0] <= b[i][0] { continue; }
            let hj = b[j][3] - b[j][1];
            let over = b[i][3].min(b[j][3]) - b[i][1].max(b[j][1]);
            if hj < 0.4 * h || hj > 2.5 * h || over < 0.5 * h.min(hj) { continue; }
            if best.map_or(true, |(x, _)| b[j][0] < x) { best = Some((b[j][0], j)); }
        }
        if let Some((_, j)) = best {
            if !has_prev[j] { next[i] = j; has_prev[j] = true; }
        }
    }
    let mut out = vec![1u32; n];
    for s in 0..n {
        if has_prev[s] { continue; }
        let mut members = vec![s];
        let mut i = s;
        while next[i] != usize::MAX && members.len() <= n { i = next[i]; members.push(i); }
        for &m in &members { out[m] = members.len() as u32; }
    }
    out
}

fn distinct(mut v: Vec<f64>) -> u32 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut n = 0;
    let mut last = f64::NEG_INFINITY;
    for x in v { if x - last > 1.0 { n += 1; last = x; } }
    n
}

pub fn features(v: &Vector, paths: &[Path], in_runs: &[bool]) -> VFeatures {
    let ps: Vec<usize> = v.paths.iter().copied().filter(|&i| shows(&paths[i])).collect();
    let (w, h) = (v.x1 - v.x0, v.y1 - v.y0);
    let mut f = VFeatures { paths: ps.len() as u32, w, h, ..Default::default() };
    let mut ys = Vec::new();
    let mut xs = Vec::new();
    let mut rx = Vec::new();
    let mut ry = Vec::new();
    let mut fill_area = 0.0;
    let mut rect_area = 0.0;
    for &i in &ps {
        let p = &paths[i];
        // rows and columns count only rules spanning at least half the cluster, as a table's do;
        // a chart's tick marks are thin lines too, but short
        if thin_h(p) { f.hlines += 1; if p.x1 - p.x0 >= 0.5 * w { ys.push((p.y0 + p.y1) / 2.0); } }
        else if thin_v(p) { f.vlines += 1; if p.y1 - p.y0 >= 0.5 * h { xs.push((p.x0 + p.x1) / 2.0); } }
        if p.rect && p.stroke {
            f.rects += 1;
            if !p.fill { f.open_rects += 1; rx.push(p.x0); ry.push(p.y0); rect_area += (p.x1 - p.x0) * (p.y1 - p.y0); }
        }
        if p.fill && !thin_h(p) && !thin_v(p) { fill_area += (p.x1 - p.x0) * (p.y1 - p.y0); }
        if p.curves > 0 { f.curved += 1; }
        if in_runs.get(i).copied().unwrap_or(false) { f.glyphs += 1; }
    }
    f.rows = distinct(ys);
    f.cols = distinct(xs);
    f.rect_cols = distinct(rx);
    f.rect_rows = distinct(ry);
    f.filled = if w * h > 0.0 { (fill_area / (w * h)).min(1.0) } else { 0.0 };
    f.rect_cover = if w * h > 0.0 { (rect_area / (w * h)).min(1.0) } else { 0.0 };
    f
}

/// The kind of every vector cluster on a page, in the order of `vectors`.
pub fn classify_page(vectors: &[Vector], paths: &[Path]) -> Vec<VKind> {
    let in_runs = glyph_runs(paths);
    vectors.iter().map(|v| classify(features(v, paths, &in_runs))).collect()
}

pub fn classify(f: VFeatures) -> VKind {
    let n = f.paths;
    let mut reasons = Vec::new();
    let done = |kind, confidence: f64, reasons, has_text: f64, f| VKind { kind, confidence, reasons, has_text, features: f };
    if n == 0 {
        return done("fill", 0.5, vec![("nothing_shows", 1.0)], 0.02, f);
    }
    // outlined text: most of its paths are letters in word-like runs
    let glyph_share = f.glyphs as f64 / n as f64;
    if glyph_share >= 0.5 {
        reasons.push(("glyphs_in_runs", glyph_share));
        return done("outlined_text", 0.9, reasons, 0.9, f);
    }
    // table grid: a lattice of rules, or a grid of stroked cells
    if (f.rows >= 3 && f.cols >= 2) || (f.open_rects >= 4 && f.rect_cols >= 2 && f.rect_rows >= 2 && f.rect_cover >= 0.7) {
        reasons.extend([("rows", f.rows as f64), ("cols", f.cols as f64), ("cells", f.open_rects as f64)]);
        return done("table_grid", 0.9, reasons, 0.02, f);
    }
    // rule: only thin lines, all one way, on one line
    let thin = f.hlines + f.vlines;
    if thin == n && ((f.vlines == 0 && f.rows <= 1) || (f.hlines == 0 && f.cols <= 1)) {
        reasons.push(("thin_lines", thin as f64));
        return done("rule", 0.9, reasons, 0.02, f);
    }
    // border: one or two open rectangles, or four thin lines framing the cluster
    if f.w >= 10.0 && f.h >= 10.0 && ((f.open_rects >= 1 && n <= 2 && f.filled < 0.2) || (f.hlines == 2 && f.vlines == 2 && n == 4)) {
        reasons.push(("frame", n as f64));
        return done("border", 0.9, reasons, 0.02, f);
    }
    // fill: a few filled shapes covering most of the box
    if f.filled >= 0.6 && n <= 3 {
        reasons.push(("filled", f.filled));
        return done("fill", 0.85, reasons, 0.02, f);
    }
    // otherwise mixed shapes: a chart or a diagram
    reasons.extend([("paths", n as f64), ("curved", f.curved as f64), ("filled", f.filled)]);
    let confidence = if n >= 5 { 0.7 } else { 0.5 };
    done("chart_or_diagram", confidence, reasons, 0.05 + 0.5 * glyph_share, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(x0: f64, y0: f64, x1: f64, y1: f64, fill: bool, stroke: bool, rect: bool, lines: u32, curves: u32) -> Path {
        Path { x0, y0, x1, y1, order: 0, fill, stroke, shading: false, white: false, annot: false, clipped: false, offpage: false, hidden: false,
               dot: false, empty: false, lines, curves, rect, closed: rect || fill }
    }

    fn cluster(paths: &[Path]) -> Vector {
        let b = paths.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p.x0), b[1].min(p.y0), b[2].max(p.x1), b[3].max(p.y1)]);
        Vector { x0: b[0], y0: b[1], x1: b[2], y1: b[3], paths: (0..paths.len()).collect(), order: 0, fill: false, stroke: true, shading: false,
                 white: false, annot: false, offpage: false, hidden: false, clipped: false, empty: false }
    }

    fn kind_of(paths: &[Path]) -> &'static str { classify_page(&[cluster(paths)], paths)[0].kind }

    #[test]
    fn one_thin_line_is_a_rule() {
        assert_eq!(kind_of(&[path(100.0, 200.0, 400.0, 201.0, false, true, false, 1, 0)]), "rule");
    }

    #[test]
    fn a_lattice_is_a_table() {
        let mut ps = Vec::new();
        for r in 0..4 { ps.push(path(100.0, 100.0 + r as f64 * 20.0, 300.0, 100.5 + r as f64 * 20.0, false, true, false, 1, 0)); }
        for c in 0..3 { ps.push(path(100.0 + c as f64 * 100.0, 100.0, 100.5 + c as f64 * 100.0, 160.0, false, true, false, 1, 0)); }
        assert_eq!(kind_of(&ps), "table_grid");
    }

    #[test]
    fn a_stroked_frame_is_a_border() {
        assert_eq!(kind_of(&[path(50.0, 50.0, 500.0, 700.0, false, true, true, 4, 0)]), "border");
    }

    #[test]
    fn a_filled_box_is_a_fill() {
        assert_eq!(kind_of(&[path(50.0, 50.0, 300.0, 120.0, true, false, true, 4, 0)]), "fill");
    }

    #[test]
    fn letters_in_a_row_are_outlined_text() {
        // five letter-shaped filled paths, 10 pt tall, close together on one baseline
        let ps: Vec<Path> = (0..5).map(|k| path(100.0 + k as f64 * 8.0, 200.0, 106.0 + k as f64 * 8.0, 210.0, true, false, false, 6, 4)).collect();
        let v = cluster(&ps);
        let k = classify_page(&[v], &ps);
        assert_eq!(k[0].kind, "outlined_text");
        assert!(k[0].has_text > 0.8);
    }
}
