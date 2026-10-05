//! The kind layer for images (plans/chunk8-kind-images.md, 8d): from an image's grey thumbnail,
//! whether it's a text image (a scan or screenshot of text), a photo, a graphic (logo, icon, chart)
//! or blank, with a confidence and the reasons, in the style of ocr.rs (D69). No model: a few pixel
//! measures and connected components, in integer arithmetic where it counts, so the same pixels
//! always give the same answer. The rules are first cuts; chunk 8e tunes them on tuning data only.

/// Longest side of the thumbnail the kind is judged on: large enough that 10 pt text in a 300 dpi
/// page scan is about 10 px tall.
pub const KIND_THUMB: u32 = 1024;

/// The measures a kind is decided from.
#[derive(Clone, Debug, Default)]
pub struct Features {
    pub w: u32, pub h: u32,
    /// Standard deviation of the grey levels.
    pub spread: f64,
    /// Grey levels (of 256) holding at least 0.1% of the pixels.
    pub levels: u32,
    /// Share of pixels near black or white (below 64 or above 192).
    pub extremes: f64,
    /// Share of "ink": pixels on the ink side of 128 (dark ink on a light ground, or light on dark), the
    /// minority side unless the two are near even (POLARITY_LO to POLARITY_HI, see features).
    pub ink: f64,
    /// The ink is light on a dark ground.
    pub inverted: bool,
    /// Connected ink components (8-connected), and of them how many are glyph-sized (2 to 60 px tall,
    /// at most 3 times as wide as tall, or thin marks like i dots).
    pub components: u32,
    pub glyphs: u32,
    /// Glyph-sized components' heights: median, and spread as the interquartile range over the median.
    pub glyph_height: f64,
    pub height_spread: f64,
    /// Share of glyph-sized components whose bottom lines up with at least 3 others (a baseline).
    pub aligned: f64,
    /// Word-like runs: 3 or more glyph-sized marks of similar height, each close to the next on one
    /// baseline (D87); how many runs, and how many marks are in them.
    pub runs: u32,
    pub word_marks: u32,
    /// The same marks for text running up or down the image (a table printed sideways on a portrait
    /// page), the larger of the two.
    pub turned_marks: u32,
}

pub struct Kind {
    pub kind: &'static str,
    pub confidence: f64,
    pub reasons: Vec<(&'static str, f64)>,
    /// That the image holds text, whatever its kind (a labelled chart is a graphic with text), 0 to 1,
    /// and the evidence (D86).
    pub has_text: f64,
    pub has_text_reasons: Vec<(&'static str, f64)>,
    pub features: Features,
}

/// Where x falls between lo and hi, 0 to 1 (hi may be below lo for a falling ramp).
fn ramp(x: f64, lo: f64, hi: f64) -> f64 { ((x - lo) / (hi - lo)).clamp(0.0, 1.0) }

/// Dark shares where the ink side is chosen by glyph count rather than by the majority.
pub const POLARITY_LO: f64 = 0.4;
pub const POLARITY_HI: f64 = 0.6;

/// A component box [x0, y0, x1, y1] that is glyph-sized: 2 to 60 px tall, at most 3 times as wide.
fn is_glyph(b: &[usize; 4]) -> bool {
    let (bw, bh) = (b[2] - b[0] + 1, b[3] - b[1] + 1);
    (2..=60).contains(&bh) && bw <= 3 * bh
}

fn glyph_count(boxes: &[[usize; 4]]) -> usize { boxes.iter().filter(|b| is_glyph(b)).count() }

/// The boxes of the 8-connected components of the ink (below 128, or 128 and up when `inverted`),
/// in the order of their first pixel.
fn components(wu: usize, hu: usize, grey: &[u8], inverted: bool) -> Vec<[usize; 4]> {
    let n = wu * hu;
    let is_ink = |g: u8| if inverted { g >= 128 } else { g < 128 };
    let mut label = vec![0u32; n];
    let mut parent: Vec<u32> = vec![0];
    fn find(p: &mut Vec<u32>, mut x: u32) -> u32 {
        while p[x as usize] != x { p[x as usize] = p[p[x as usize] as usize]; x = p[x as usize]; }
        x
    }
    for y in 0..hu {
        for x in 0..wu {
            if !is_ink(grey[y * wu + x]) { continue; }
            let mut nb = [0u32; 4];
            let mut k = 0;
            if x > 0 && label[y * wu + x - 1] != 0 { nb[k] = label[y * wu + x - 1]; k += 1; }
            if y > 0 {
                for dx in [-1isize, 0, 1] {
                    let xx = x as isize + dx;
                    if xx >= 0 && (xx as usize) < wu && label[(y - 1) * wu + xx as usize] != 0 { nb[k] = label[(y - 1) * wu + xx as usize]; k += 1; }
                }
            }
            if k == 0 {
                let id = parent.len() as u32;
                parent.push(id);
                label[y * wu + x] = id;
            } else {
                let mut root = find(&mut parent, nb[0]);
                for &o in &nb[1..k] {
                    let r = find(&mut parent, o);
                    if r != root { let (a, b) = (root.min(r), root.max(r)); parent[b as usize] = a; root = a; }
                }
                label[y * wu + x] = root;
            }
        }
    }
    let mut boxes: std::collections::BTreeMap<u32, [usize; 4]> = std::collections::BTreeMap::new();
    for y in 0..hu {
        for x in 0..wu {
            let l = label[y * wu + x];
            if l == 0 { continue; }
            let r = find(&mut parent, l);
            let b = boxes.entry(r).or_insert([x, y, x, y]);
            b[0] = b[0].min(x); b[1] = b[1].min(y); b[2] = b[2].max(x); b[3] = b[3].max(y);
        }
    }
    boxes.into_values().collect()
}

pub fn features(w: u32, h: u32, grey: &[u8]) -> Features {
    let n = (w as usize) * (h as usize);
    let mut f = Features { w, h, ..Default::default() };
    if n == 0 || grey.len() < n { return f; }
    let mut hist = [0u64; 256];
    for &g in &grey[..n] { hist[g as usize] += 1; }
    let sum: u64 = hist.iter().enumerate().map(|(g, c)| g as u64 * c).sum();
    let mean = sum as f64 / n as f64;
    let var: f64 = hist.iter().enumerate().map(|(g, &c)| c as f64 * (g as f64 - mean).powi(2)).sum::<f64>() / n as f64;
    f.spread = var.sqrt();
    f.levels = hist.iter().filter(|&&c| c * 1000 >= n as u64).count() as u32;
    let ext: u64 = hist[..64].iter().sum::<u64>() + hist[193..].iter().sum::<u64>();
    f.extremes = ext as f64 / n as f64;
    let dark: u64 = hist[..128].iter().sum();
    // Which side of 128 is ink. By the majority when it's clear; when dark and light are near even,
    // by which side makes more glyph-sized marks, since text is many small marks on a ground. A page
    // scan with a black scanner margin over half the image is dark ink on paper, not paper on black
    // (a Sodir table scan: 47 components the majority way, 436 the right way; results/kinds-sparse.md).
    let share = dark as f64 / n as f64;
    let (wu, hu) = (w as usize, h as usize);
    let majority = dark * 2 > n as u64;
    let (inverted, boxes) = if (POLARITY_LO..=POLARITY_HI).contains(&share) {
        let (a, b) = (components(wu, hu, grey, majority), components(wu, hu, grey, !majority));
        if glyph_count(&b) > glyph_count(&a) { (!majority, b) } else { (majority, a) }
    } else {
        (majority, components(wu, hu, grey, majority))
    };
    f.inverted = inverted;
    let ink_count = if f.inverted { n as u64 - dark } else { dark };
    f.ink = ink_count as f64 / n as f64;
    f.components = boxes.len() as u32;
    let glyphs: Vec<[usize; 4]> = boxes.into_iter().filter(is_glyph).collect();
    f.glyphs = glyphs.len() as u32;
    if !glyphs.is_empty() {
        let mut hs: Vec<usize> = glyphs.iter().map(|b| b[3] - b[1] + 1).collect();
        hs.sort_unstable();
        let q = |p: usize| hs[(hs.len() - 1) * p / 4] as f64;
        f.glyph_height = q(2);
        f.height_spread = if q(2) > 0.0 { (q(3) - q(1)) / q(2) } else { 0.0 };
        // baselines: bottoms within one pixel, counted by bottom row
        let mut bottoms = vec![0u32; hu + 2];
        for b in &glyphs { bottoms[b[3] + 1] += 1; }
        let aligned = glyphs.iter().filter(|b| bottoms[b[3]] + bottoms[b[3] + 1] + bottoms[b[3] + 2] >= 4).count();
        f.aligned = aligned as f64 / glyphs.len() as f64;
        let (runs, marks) = word_runs(&glyphs);
        f.runs = runs;
        f.word_marks = marks;
        // reading upwards, a letter's foot faces right; reading downwards, left. Each turned so the
        // reading runs left to right and the foot is at the bottom, then counted the same way.
        let up: Vec<[usize; 4]> = glyphs.iter().map(|b| [hu - 1 - b[3], b[0], hu - 1 - b[1], b[2]]).collect();
        let down: Vec<[usize; 4]> = glyphs.iter().map(|b| [b[1], wu - 1 - b[2], b[3], wu - 1 - b[0]]).collect();
        f.turned_marks = word_runs(&up).1.max(word_runs(&down).1);
    }
    f
}

/// Word-like runs among glyph boxes [x0, y0, x1, y1]: each mark is linked to the nearest mark to its
/// right whose bottom is within a pixel or so (a sixth of the height), whose height is half to twice
/// its own, and which starts within about one height of its right edge. Chains of 3 or more count.
fn word_runs(glyphs: &[[usize; 4]]) -> (u32, u32) {
    let len = run_lengths(glyphs);
    let marks = len.iter().filter(|&&l| l >= 3).count() as u32;
    // a run of length l gives l marks of length l, so the marks of each length over it count runs
    let mut by_len: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    for &l in len.iter().filter(|&&l| l >= 3) { *by_len.entry(l).or_default() += 1; }
    let runs = by_len.iter().map(|(l, c)| c / l).sum();
    (runs, marks)
}

/// For each mark, the length of the word-like run it belongs to (1 when it's in none), by the
/// linking rule of word_runs.
fn run_lengths(glyphs: &[[usize; 4]]) -> Vec<u32> {
    let n = glyphs.len();
    let mut by_bottom: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, b) in glyphs.iter().enumerate() { by_bottom.entry(b[3]).or_default().push(i); }
    for v in by_bottom.values_mut() { v.sort_by_key(|&i| (glyphs[i][0], i)); }
    let mut next = vec![usize::MAX; n];
    let mut has_prev = vec![false; n];
    for (i, b) in glyphs.iter().enumerate() {
        let h = b[3] - b[1] + 1;
        let tol = (h / 6).max(1);
        let reach = b[2] + h + 1;
        let mut best: Option<(usize, usize)> = None;
        for (_, v) in by_bottom.range(b[3].saturating_sub(tol)..=b[3] + tol) {
            // the first mark starting right of this one's left edge
            let k = v.partition_point(|&j| glyphs[j][0] <= b[0]);
            for &j in v[k..].iter().take(4) {
                let c = &glyphs[j];
                if c[0] > reach { break; }
                let hj = c[3] - c[1] + 1;
                if hj * 2 < h || hj > h * 2 || c[0] + 1 < b[2] { continue; }
                if best.map_or(true, |(x, _)| c[0] < x) { best = Some((c[0], j)); }
            }
        }
        if let Some((_, j)) = best {
            if !has_prev[j] { next[i] = j; has_prev[j] = true; }
        }
    }
    let mut out = vec![1u32; n];
    for start in 0..n {
        if has_prev[start] { continue; }
        let mut members = vec![start];
        let mut i = start;
        while next[i] != usize::MAX && members.len() <= n { i = next[i]; members.push(i); }
        for &m in &members { out[m] = members.len() as u32; }
    }
    out
}

/// The sparse-page evidence: the share of glyph-sized marks on shared baselines, and how many such
/// marks, each as a ramp (from, to). Tuned on govdocs1 003 and the Sodir tune pages.
pub const SPARSE_ALIGNED: (f64, f64) = (0.35, 0.5);
pub const SPARSE_LINED: (f64, f64) = (5.0, 10.0);
/// ... and only on a nearly empty image (ink falling from SPARSE_INK.0 to SPARSE_INK.1, so at most a
/// few per cent) whose thumbnail is page-sized (its long side from SPARSE_SIDE.0 to SPARSE_SIDE.1 px).
pub const SPARSE_INK: (f64, f64) = (0.03, 0.015);
pub const SPARSE_SIDE: (f64, f64) = (400.0, 600.0);

/// The kind of an image from its thumbnail.
pub fn classify(w: u32, h: u32, grey: &[u8]) -> Kind {
    let f = features(w, h, grey);
    let mut reasons = Vec::new();
    // has-text evidence whatever the kind: a sparse page (a heading or two on a scan) has few marks
    // in word runs, or none when its letters are spaced out, but most of its glyph-sized marks sit on
    // shared baselines. Only for a page-sized, nearly empty image: on govdocs1 003 the same evidence
    // without those limits held 2,202 more images, small inked logos and icons (results/kinds-sparse.md).
    let lined_marks = f.aligned * f.glyphs as f64;
    let sparse = ramp(f.aligned, SPARSE_ALIGNED.0, SPARSE_ALIGNED.1) * ramp(lined_marks, SPARSE_LINED.0, SPARSE_LINED.1)
        * ramp(f.ink, SPARSE_INK.0, SPARSE_INK.1) * ramp(f.w.max(f.h) as f64, SPARSE_SIDE.0, SPARSE_SIDE.1);
    let round = |x: f64| (x * 1000.0).round() / 1000.0;
    // blank: almost no variation, or no ink at all; still has-text when a line or two sits on it
    if f.spread < 4.0 || f.ink < 0.0005 {
        reasons.push(("flat", f.spread));
        let has_text = if sparse > 0.0 { round(0.05 + 0.9 * sparse) } else { 0.0 };
        return Kind { kind: "blank", confidence: 0.9, reasons, has_text, has_text_reasons: vec![("flat", f.spread), ("sparse", round(sparse))], features: f };
    }
    // text: two-tone, some ink, glyph-sized marks that sit on shared baselines. On the constructed
    // tune split the baseline share separates them (text images 0.66 and up, median 0.94; logos 0;
    // photos at most 0.57, median 0.02), so its ramp sits in that gap. Heights vary a lot in real
    // text (ascenders, x-height, dots), so only extreme spreads count against it.
    let two_tone = ramp(f.extremes, 0.5, 0.8);
    let inkish = ramp(f.ink, 0.002, 0.006) * ramp(f.ink, 0.5, 0.3);
    let many = ramp(f.glyphs as f64, 8.0, 30.0);
    let even = ramp(f.height_spread, 4.0, 2.5);
    let lined = ramp(f.aligned, 0.5, 0.75);
    let text = two_tone * inkish * many * even * lined;
    // photo: many grey levels and little two-tone area
    let photo = ramp(f.levels as f64, 32.0, 96.0) * ramp(f.extremes, 0.8, 0.5);
    // graphic: two-tone or few levels, but not text-like
    let graphic = ramp(f.levels as f64, 96.0, 24.0).max(two_tone) * (1.0 - text);
    let (kind, score) = [("text", text), ("photo", photo), ("graphic", graphic)]
        .into_iter().fold(("graphic", -1.0), |a, b| if b.1 > a.1 { b } else { a });
    reasons.extend([("two_tone", two_tone), ("ink", inkish), ("glyphs", many), ("even_heights", even), ("baselines", lined), ("levels", f.levels as f64)]);
    // confidence: the winning score, lowered when the runner-up is close
    let second = [text, photo, graphic].iter().copied().filter(|&s| s < score).fold(0.0, f64::max);
    let confidence = (score * (0.5 + 0.5 * (score - second).max(0.0))).clamp(0.05, 0.95);
    // has-text: marks in word-like runs. Tuned on 003 against labels from each image's own pixels
    // (tools/label_images.py): the best single cut is about 20 to 30 marks for graphics, 60 to 90
    // for photos (their texture makes short runs by chance) and lower for text images, so each ramp
    // is centred there. The confidence follows the ramp, from 0.05 to 0.95.
    // Text printed sideways counts too, on the photo ramp: the uprights of charts and tables line up
    // into turned runs by chance, as a photo's texture does (on 003, 17 to 77 turned marks on graphics
    // without text; 139 and 171 on sideways Sodir tables). Sparse pages count for every kind but a photo.
    let (marks, turned) = (f.word_marks as f64, f.turned_marks as f64);
    let words = match kind {
        "photo" => ramp(marks.max(turned), 40.0, 120.0),
        "text" => ramp(marks, 4.0, 30.0).max(ramp(turned, 40.0, 120.0)).max(sparse),
        _ => ramp(marks, 8.0, 40.0).max(ramp(turned, 40.0, 120.0)).max(sparse),
    };
    let has_text = 0.05 + 0.9 * words;
    let has_text_reasons = vec![("word_marks", f.word_marks as f64), ("turned_marks", f.turned_marks as f64), ("runs", f.runs as f64),
                                ("sparse", round(sparse)), ("photo", if kind == "photo" { 1.0 } else { 0.0 })];
    Kind { kind, confidence: (confidence * 1000.0).round() / 1000.0, reasons, has_text: (has_text * 1000.0).round() / 1000.0, has_text_reasons, features: f }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white page with rows of small black blocks, like lines of text.
    fn text_like(w: u32, h: u32) -> Vec<u8> {
        let mut g = vec![255u8; (w * h) as usize];
        for line in 0..(h / 20) {
            let top = line * 20 + 5;
            for c in 0..(w / 8) {
                let left = c * 8 + 1;
                let tall = if c % 5 == 0 { 12 } else { 9 };
                for y in top + (12 - tall)..top + 12 {
                    for x in left..left + 5 { g[(y * w + x) as usize] = 0; }
                }
            }
        }
        g
    }

    #[test]
    fn runs_of_marks_are_words() {
        // 5 marks on a baseline, close together: one run of 5; a lone mark far off: no run
        let mut g = vec![[0usize; 4]; 0];
        for k in 0..5 { g.push([10 + k * 8, 10, 15 + k * 8, 19]); }
        g.push([200, 50, 205, 59]);
        assert_eq!(word_runs(&g), (1, 5));
    }

    #[test]
    fn a_flat_image_is_blank() {
        assert_eq!(classify(50, 50, &vec![240u8; 2500]).kind, "blank");
    }

    #[test]
    fn rows_of_glyphs_are_text() {
        let k = classify(400, 200, &text_like(400, 200));
        assert_eq!(k.kind, "text", "{:?}", k.features);
        assert!(k.features.aligned > 0.9 && k.features.glyphs > 100);
        assert!(k.has_text > 0.9 && k.features.word_marks > 100);
    }

    #[test]
    fn a_smooth_gradient_is_a_photo() {
        let (w, h) = (256u32, 256u32);
        let g: Vec<u8> = (0..w * h).map(|i| ((i % w + i / w) / 2) as u8).collect();
        assert_eq!(classify(w, h, &g).kind, "photo");
    }

    #[test]
    fn one_big_shape_is_a_graphic() {
        let (w, h) = (100u32, 100u32);
        let g: Vec<u8> = (0..w * h).map(|i| { let (x, y) = (i % w, i / w); if (20..80).contains(&x) && (20..80).contains(&y) { 0 } else { 255 } }).collect();
        assert_eq!(classify(w, h, &g).kind, "graphic");
    }
}
