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
    /// Share of "ink": pixels on the minority side of 128 (dark ink on a light ground, or light on dark).
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
}

pub struct Kind {
    pub kind: &'static str,
    pub confidence: f64,
    pub reasons: Vec<(&'static str, f64)>,
    pub features: Features,
}

/// Where x falls between lo and hi, 0 to 1 (hi may be below lo for a falling ramp).
fn ramp(x: f64, lo: f64, hi: f64) -> f64 { ((x - lo) / (hi - lo)).clamp(0.0, 1.0) }

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
    f.inverted = dark * 2 > n as u64;
    let ink_count = if f.inverted { n as u64 - dark } else { dark };
    f.ink = ink_count as f64 / n as f64;

    // connected components of the ink
    let is_ink = |g: u8| if f.inverted { g >= 128 } else { g < 128 };
    let mut label = vec![0u32; n];
    let mut parent: Vec<u32> = vec![0];
    fn find(p: &mut Vec<u32>, mut x: u32) -> u32 {
        while p[x as usize] != x { p[x as usize] = p[p[x as usize] as usize]; x = p[x as usize]; }
        x
    }
    let (wu, hu) = (w as usize, h as usize);
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
    // each component's box
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
    f.components = boxes.len() as u32;
    let glyphs: Vec<[usize; 4]> = boxes.values().copied().filter(|b| {
        let (bw, bh) = (b[2] - b[0] + 1, b[3] - b[1] + 1);
        (2..=60).contains(&bh) && bw <= 3 * bh
    }).collect();
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
    }
    f
}

/// The kind of an image from its thumbnail.
pub fn classify(w: u32, h: u32, grey: &[u8]) -> Kind {
    let f = features(w, h, grey);
    let mut reasons = Vec::new();
    // blank: almost no variation, or no ink at all
    if f.spread < 4.0 || f.ink < 0.0005 {
        reasons.push(("flat", f.spread));
        return Kind { kind: "blank", confidence: 0.9, reasons, features: f };
    }
    // text: two-tone, a moderate amount of ink, many glyph-sized marks of similar height on baselines
    let two_tone = ramp(f.extremes, 0.6, 0.9);
    let inkish = ramp(f.ink, 0.005, 0.02) * ramp(f.ink, 0.5, 0.3);
    let many = ramp(f.glyphs as f64, 5.0, 40.0);
    let even = ramp(f.height_spread, 1.0, 0.4);
    let lined = ramp(f.aligned, 0.2, 0.6);
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
    Kind { kind, confidence: (confidence * 1000.0).round() / 1000.0, reasons, features: f }
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
    fn a_flat_image_is_blank() {
        assert_eq!(classify(50, 50, &vec![240u8; 2500]).kind, "blank");
    }

    #[test]
    fn rows_of_glyphs_are_text() {
        let k = classify(400, 200, &text_like(400, 200));
        assert_eq!(k.kind, "text", "{:?}", k.features);
        assert!(k.features.aligned > 0.9 && k.features.glyphs > 100);
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
