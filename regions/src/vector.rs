//! Vector marks: every painted path and shading, as a box on the page, and the clusters they form.
//! Paths that touch are one region of the map (a table's rules, a chart's bars and axes); a path
//! that covers much of the page, such as a background fill, stays a region of its own so it doesn't
//! pull everything on the page into one cluster.

/// One painted path or shading, placed on the page (points from the top-left of the visible page).
#[derive(Clone, Debug)]
pub struct Path {
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    pub order: u32,
    pub fill: bool,
    pub stroke: bool,
    /// A shading (the sh operator): it paints its whole clip.
    pub shading: bool,
    /// Painted in white (fill and stroke, as far as they're painted), so it shows nothing on a white page.
    pub white: bool,
    pub annot: bool,
    /// Its clip cut part of it.
    pub clipped: bool,
    /// Nothing of it is on the visible page; the box is the painted box.
    pub offpage: bool,
    /// Its clip hides all of it; the box is its part on the page.
    pub hidden: bool,
    /// All its points are one point and its stroke is a round-capped dot (ISO 32000-1, 8.5.3.2).
    pub dot: bool,
    /// All its points are one point, it isn't filled (a fill paints the pixel under it) and it isn't a
    /// dot, so it paints nothing.
    pub empty: bool,
    /// Its shape, for the vector kinds (chunk 9; not in the JSON): straight and curved segments, whether
    /// it was built with the `re` operator, and whether any subpath is closed.
    pub lines: u32,
    pub curves: u32,
    pub rect: bool,
    pub closed: bool,
    /// Distinct rows of its horizontal straight segments and columns of its vertical ones that span
    /// at least half the path's own width or height (a table drawn as one path has several).
    pub seg_rows: u32,
    pub seg_cols: u32,
}

/// Touching paths, as one region.
#[derive(Clone, Debug)]
pub struct Vector {
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    /// Indexes into `Page::paths`, in drawing order.
    pub paths: Vec<usize>,
    pub order: u32,
    pub fill: bool, pub stroke: bool, pub shading: bool,
    /// Every path in it is white.
    pub white: bool,
    pub annot: bool,
    pub offpage: bool,
    pub hidden: bool,
    /// Any path in it was cut by its clip.
    pub clipped: bool,
    /// Every path in it paints nothing.
    pub empty: bool,
}

/// One segment of a path, in default user space after the transform.
#[derive(Clone, Copy, Debug)]
pub enum Seg {
    Line((f64, f64), (f64, f64)),
    /// A curve's extent (x0, y0, x1, y1) and its two ends.
    Curve([f64; 4], (f64, f64), (f64, f64)),
}

impl Seg {
    fn ends(&self) -> ((f64, f64), (f64, f64)) {
        match *self { Seg::Line(p, q) => (p, q), Seg::Curve(_, p, q) => (p, q) }
    }
}

/// The box a stroke paints, from its subpaths (segments, whether each is closed, and its start),
/// half the line width and the line cap (0 butt, 1 round, 2 square). A subpath whose points are all
/// one point paints a dot only with round caps, and only when it's closed or has a segment; a lone
/// moveto paints nothing (ISO 32000-1, 8.5.3.2). A line segment grows by half the width across
/// itself only; a join adds half the width around its corner (a miter's point past that is left
/// out); an open end grows along the line only with round or square caps. A curve grows by half the
/// width all round.
pub fn stroke_box(subs: &[(Vec<Seg>, bool, (f64, f64))], hw: f64, cap: i64) -> Option<[f64; 4]> {
    let mut b: Option<[f64; 4]> = None;
    let mut add = |x: f64, y: f64| match &mut b {
        Some(v) => { v[0] = v[0].min(x); v[1] = v[1].min(y); v[2] = v[2].max(x); v[3] = v[3].max(y); }
        None => b = Some([x, y, x, y]),
    };
    let at = |v: (f64, f64), p: (f64, f64)| (v.0 - p.0).abs() < 1e-9 && (v.1 - p.1).abs() < 1e-9;
    for (segs, closed, p0) in subs {
        let point = segs.iter().all(|s| match *s {
            Seg::Line(p, q) => at(p, *p0) && at(q, *p0),
            Seg::Curve(c, _, _) => at((c[0], c[1]), *p0) && at((c[2], c[3]), *p0),
        });
        if point {
            if cap == 1 && (*closed || !segs.is_empty()) { add(p0.0 - hw, p0.1 - hw); add(p0.0 + hw, p0.1 + hw); }
            continue;
        }
        for s in segs {
            match *s {
                Seg::Line(p, q) => {
                    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                    let len = (dx * dx + dy * dy).sqrt();
                    if len < 1e-9 {
                        add(p.0 - hw, p.1 - hw); add(p.0 + hw, p.1 + hw);
                        continue;
                    }
                    let (nx, ny) = (-dy / len * hw, dx / len * hw);
                    for (x, y) in [p, q] { add(x + nx, y + ny); add(x - nx, y - ny); }
                }
                Seg::Curve(c, _, _) => { add(c[0] - hw, c[1] - hw); add(c[2] + hw, c[3] + hw); }
            }
        }
        // joins between segments, and the closing join
        let n = segs.len();
        let joins = if *closed { n } else { n.saturating_sub(1) };
        for k in 0..joins {
            let (_, v) = segs[k].ends();
            add(v.0 - hw, v.1 - hw); add(v.0 + hw, v.1 + hw);
        }
        if !*closed && cap != 0 && n > 0 {
            for v in [segs[0].ends().0, segs[n - 1].ends().1] { add(v.0 - hw, v.1 - hw); add(v.0 + hw, v.1 + hw); }
        }
    }
    b
}

/// A path's own grid: distinct rows (to a point) of its horizontal straight segments spanning at least
/// half the path's box width, and columns of its vertical ones spanning half its height. In the
/// path's user space before placement, so a rotated page swaps the two, which the grid test doesn't mind.
pub fn seg_grid(subs: &[(Vec<Seg>, bool, (f64, f64))], b: &[f64; 4]) -> (u32, u32) {
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    let (mut ys, mut xs) = (Vec::new(), Vec::new());
    for (segs, _, _) in subs {
        for s in segs {
            if let Seg::Line(p, q) = *s {
                let (dx, dy) = ((q.0 - p.0).abs(), (q.1 - p.1).abs());
                if dy < 0.5 && dx >= 0.5 * w && dx > 1.0 { ys.push(p.1); }
                if dx < 0.5 && dy >= 0.5 * h && dy > 1.0 { xs.push(p.0); }
            }
        }
    }
    let count = |mut v: Vec<f64>| {
        v.sort_by(|a, c| a.partial_cmp(c).unwrap());
        let (mut n, mut last) = (0u32, f64::NEG_INFINITY);
        for x in v { if x - last > 1.0 { n += 1; last = x; } }
        n
    };
    (count(ys), count(xs))
}

/// Paths closer than this, in points, are one cluster.
pub const TOUCH_PT: f64 = 1.0;
/// A path covering more than this share of the page stays a cluster of its own.
pub const LARGE_SHARE: f64 = 0.25;
/// Grid cell for finding neighbours, in points.
const CELL: f64 = 36.0;

/// Where a cubic Bezier's coordinate reaches its extremes: the ends and the roots of its derivative.
pub fn cubic_range(p0: f64, p1: f64, p2: f64, p3: f64) -> (f64, f64) {
    let at = |t: f64| {
        let u = 1.0 - t;
        u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
    };
    let (mut lo, mut hi) = (p0.min(p3), p0.max(p3));
    // B'(t)/3 = a t^2 + b t + c
    let (a, b, c) = (p1 - p0 - 2.0 * (p2 - p1) + (p3 - p2), 2.0 * ((p2 - p1) - (p1 - p0)), p1 - p0);
    let mut roots = Vec::with_capacity(2);
    if a.abs() < 1e-12 {
        if b.abs() > 1e-12 { roots.push(-c / b); }
    } else {
        let d = b * b - 4.0 * a * c;
        if d >= 0.0 {
            let s = d.sqrt();
            roots.push((-b + s) / (2.0 * a));
            roots.push((-b - s) / (2.0 * a));
        }
    }
    for t in roots.into_iter().filter(|t| *t > 0.0 && *t < 1.0) {
        let v = at(t);
        lo = lo.min(v);
        hi = hi.max(v);
    }
    (lo, hi)
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i { parent[i] = parent[parent[i]]; i = parent[i]; }
    i
}

/// Paths to clusters. Offpage paths and large ones are each their own cluster; the rest join
/// when their boxes come within TOUCH_PT of each other.
pub fn cluster(paths: &[Path], width: f64, height: f64) -> Vec<Vector> {
    let page = (width * height).max(1e-9);
    let alone = |p: &Path| p.offpage || p.hidden || (p.x1 - p.x0) * (p.y1 - p.y0) > LARGE_SHARE * page;
    let mut parent: Vec<usize> = (0..paths.len()).collect();
    let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    for (i, p) in paths.iter().enumerate() {
        if alone(p) { continue; }
        let (cx0, cy0) = (((p.x0 - TOUCH_PT) / CELL).floor() as i64, ((p.y0 - TOUCH_PT) / CELL).floor() as i64);
        let (cx1, cy1) = (((p.x1 + TOUCH_PT) / CELL).floor() as i64, ((p.y1 + TOUCH_PT) / CELL).floor() as i64);
        for cx in cx0..=cx1 {
            for cy in cy0..=cy1 {
                let cell = grid.entry((cx, cy)).or_default();
                for &j in cell.iter() {
                    let q = &paths[j];
                    let near = p.x0 <= q.x1 + TOUCH_PT && q.x0 <= p.x1 + TOUCH_PT && p.y0 <= q.y1 + TOUCH_PT && q.y0 <= p.y1 + TOUCH_PT;
                    if near {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        if a != b { parent[a.max(b)] = a.min(b); }
                    }
                }
                cell.push(i);
            }
        }
    }
    let mut out: Vec<Vector> = Vec::new();
    let mut slot: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (i, p) in paths.iter().enumerate() {
        let root = find(&mut parent, i);
        match slot.get(&root) {
            Some(&k) => {
                let v = &mut out[k];
                v.x0 = v.x0.min(p.x0); v.y0 = v.y0.min(p.y0); v.x1 = v.x1.max(p.x1); v.y1 = v.y1.max(p.y1);
                v.paths.push(i);
                v.order = v.order.min(p.order);
                v.fill |= p.fill; v.stroke |= p.stroke; v.shading |= p.shading;
                v.white &= p.white; v.empty &= p.empty; v.annot &= p.annot; v.clipped |= p.clipped;
            }
            None => {
                slot.insert(root, out.len());
                out.push(Vector { x0: p.x0, y0: p.y0, x1: p.x1, y1: p.y1, paths: vec![i], order: p.order,
                    fill: p.fill, stroke: p.stroke, shading: p.shading, white: p.white, annot: p.annot, offpage: p.offpage, hidden: p.hidden, clipped: p.clipped, empty: p.empty });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(x0: f64, y0: f64, x1: f64, y1: f64, order: u32) -> Path {
        Path { x0, y0, x1, y1, order, fill: true, stroke: false, shading: false, white: false, annot: false, clipped: false, offpage: false, hidden: false, dot: false, empty: false, lines: 4, curves: 0, rect: true, closed: true, seg_rows: 2, seg_cols: 2 }
    }

    #[test]
    fn a_curve_reaches_past_its_ends() {
        // a symmetric arch from 0 to 0 with control points at 1: the peak is at 0.75
        let (lo, hi) = cubic_range(0.0, 1.0, 1.0, 0.0);
        assert!(lo.abs() < 1e-12 && (hi - 0.75).abs() < 1e-12);
        // monotone: just the ends
        assert_eq!(cubic_range(0.0, 1.0, 2.0, 3.0), (0.0, 3.0));
    }

    #[test]
    fn touching_paths_are_one_cluster() {
        // a table: two rules and a column line that meets them, plus a far-away box
        let ps = [path(50.0, 100.0, 500.0, 101.0, 0), path(50.0, 200.0, 500.0, 201.0, 1),
                  path(275.0, 100.0, 276.0, 201.0, 2), path(50.0, 600.0, 80.0, 630.0, 3)];
        let vs = cluster(&ps, 600.0, 800.0);
        assert_eq!(vs.len(), 2);
        assert!(vs[0].paths == [0, 1, 2] && (vs[0].y1 - 201.0).abs() < 1e-9 && vs[1].paths == [3]);
    }

    #[test]
    fn a_background_doesnt_swallow_the_page() {
        let ps = [path(0.0, 0.0, 600.0, 800.0, 0), path(50.0, 100.0, 500.0, 101.0, 1), path(50.0, 101.5, 500.0, 102.5, 2)];
        let vs = cluster(&ps, 600.0, 800.0);
        assert!(vs.len() == 2 && vs[0].paths == [0] && vs[1].paths == [1, 2]);
    }

    #[test]
    fn a_butt_line_grows_across_itself_only() {
        let line = vec![(vec![Seg::Line((100.0, 400.0), (300.0, 400.0))], false, (100.0, 400.0))];
        assert_eq!(stroke_box(&line, 1.0, 0), Some([100.0, 399.0, 300.0, 401.0]));
        // round or square caps reach past the ends
        assert_eq!(stroke_box(&line, 1.0, 2), Some([99.0, 399.0, 301.0, 401.0]));
        // a closed rectangle's corners are joins
        let (a, b, c, d) = ((10.0, 10.0), (50.0, 10.0), (50.0, 30.0), (10.0, 30.0));
        let rect = vec![(vec![Seg::Line(a, b), Seg::Line(b, c), Seg::Line(c, d), Seg::Line(d, a)], true, a)];
        assert_eq!(stroke_box(&rect, 0.5, 0), Some([9.5, 9.5, 50.5, 30.5]));
    }

    #[test]
    fn a_point_is_a_dot_only_with_round_caps() {
        let p = (20.0, 30.0);
        let closed = vec![(vec![], true, p)];
        assert_eq!(stroke_box(&closed, 0.5, 1), Some([19.5, 29.5, 20.5, 30.5]));
        assert_eq!(stroke_box(&closed, 0.5, 0), None);
        assert_eq!(stroke_box(&closed, 0.5, 2), None);
        // a line to the same point is one too; a lone moveto isn't
        assert_eq!(stroke_box(&vec![(vec![Seg::Line(p, p)], false, p)], 0.5, 1), Some([19.5, 29.5, 20.5, 30.5]));
        assert_eq!(stroke_box(&vec![(vec![], false, p)], 0.5, 1), None);
    }

    #[test]
    fn a_gap_wider_than_a_point_splits() {
        let ps = [path(50.0, 100.0, 100.0, 110.0, 0), path(101.5, 100.0, 150.0, 110.0, 1)];
        assert_eq!(cluster(&ps, 600.0, 800.0).len(), 2);
    }
}
