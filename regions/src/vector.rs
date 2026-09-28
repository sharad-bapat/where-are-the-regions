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
                v.white &= p.white; v.annot &= p.annot;
            }
            None => {
                slot.insert(root, out.len());
                out.push(Vector { x0: p.x0, y0: p.y0, x1: p.x1, y1: p.y1, paths: vec![i], order: p.order,
                    fill: p.fill, stroke: p.stroke, shading: p.shading, white: p.white, annot: p.annot, offpage: p.offpage, hidden: p.hidden });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(x0: f64, y0: f64, x1: f64, y1: f64, order: u32) -> Path {
        Path { x0, y0, x1, y1, order, fill: true, stroke: false, shading: false, white: false, annot: false, clipped: false, offpage: false, hidden: false }
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
    fn a_gap_wider_than_a_point_splits() {
        let ps = [path(50.0, 100.0, 100.0, 110.0, 0), path(101.5, 100.0, 150.0, 110.0, 1)];
        assert_eq!(cluster(&ps, 600.0, 800.0).len(), 2);
    }
}
