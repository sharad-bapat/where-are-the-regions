//! regions: where are the regions?
//!
//! Every image a born-digital PDF draws, with its box on the page, next to every word it draws.
//! The word reader is wordbox (Where is the text?), copied at d544ff4 so the tools stay independent;
//! what's new here is marked "regions:".
//!
//! wordbox's own notes follow.
//!
//! Reads what a born-digital PDF draws and reports each glyph as Unicode text, in drawing order.
//! It never renders anything and never infers meaning: every character comes from the file's own
//! records (fonts, encodings, ToUnicode maps), by rules fixed in the PDF spec.
//!
//! The parser (object index, page tree, decryption, stream filters) is copied from scan-or-text,
//! so the two tools stay independent.
use std::collections::HashMap;

mod ccitt;
mod cff;
mod cmap;
// regions: images to OCR regions
pub mod ocr;
pub use ocr::Region;
mod font;
mod outline;
pub mod map;
/// Image pixels as grey thumbnails, for the kind layer (chunk 8).
pub mod pixels;
/// The kind of each image (text image, photo, graphic, blank) from its pixels.
pub mod kind;
/// The kind of each vector cluster (rule, border, table grid, fill, chart or diagram, outlined text).
pub mod vkind;
pub use map::{Entry, Line};
pub mod vector;
pub use vector::{Path, Vector};
mod tables;
mod truetype;
mod type1;

pub use font::Kind;

const MAX_FORM_DEPTH: usize = 8;

// The PDF reading itself (byte helpers, values, filters, the object index, decryption, the page
// tree) is pdf-core's, shared with scan-or-text and wordbox.
pub(crate) use pdf_core::*;

/// What a colour space's components mean for telling white: 1 gray (DeviceGray, CalGray, ICCBased
/// with /N 1), 3 RGB (DeviceRGB, CalRGB, ICCBased /N 3), 4 CMYK (DeviceCMYK, ICCBased /N 4), 0 any
/// other (Lab, Separation, DeviceN, Indexed, Pattern), which is never taken as white.
fn cs_kind(pdf: &Pdf, resources: Option<&[u8]>, name: &[u8]) -> u8 {
    match name {
        b"DeviceGray" | b"G" => return 1,
        b"DeviceRGB" | b"RGB" => return 3,
        b"DeviceCMYK" | b"CMYK" => return 4,
        _ => {}
    }
    let spaces = match resources.and_then(|r| get(r, b"/ColorSpace")).and_then(|v| pdf.resolve(&v)) { Some(d) => d, None => return 0 };
    let mut key = vec![b'/'];
    key.extend_from_slice(name);
    let arr = match get(&spaces, &key).map(|v| pdf.direct(v)) {
        Some(Val::Name(n)) if matches!(n.as_slice(), b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK") => return cs_kind(pdf, None, &n),
        Some(Val::Array(a)) => a,
        _ => return 0,
    };
    // [/Family ...] (the bytes inside the brackets): the first element names the family, the second is its parameters
    let first = skip_ws(&arr, 0);
    match parse_val(&arr, first) {
        Val::Name(f) if f == b"CalGray" => 1,
        Val::Name(f) if f == b"CalRGB" => 3,
        Val::Name(f) if f == b"ICCBased" => {
            let icc = match pdf.resolve(&parse_val(&arr, skip_val(&arr, first))) { Some(d) => d, None => return 0 };
            match get(&icc, b"/N").map(|v| pdf.direct(v)) {
                Some(Val::Num(n)) if n == 1.0 => 1,
                Some(Val::Num(n)) if n == 3.0 => 3,
                Some(Val::Num(n)) if n == 4.0 => 4,
                _ => 0,
            }
        }
        _ => 0,
    }
}

/// Whether sc or scn operands are white in a colour space of this kind (see cs_kind).
fn is_white(kind: u8, ops: &[Tok]) -> bool {
    let k = kind as usize;
    if k == 0 || ops.len() < k || !ops[ops.len() - k..].iter().all(|t| matches!(t, Tok::Num(_))) { return false; }
    let v = &ops[ops.len() - k..];
    if kind == 4 { v.iter().all(|t| num(t) <= 0.0) } else { v.iter().all(|t| num(t) >= 1.0) }
}

// ---------- the content-stream interpreter ----------

type M = [f64; 6];
const IDENT: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
fn mul(a: &M, b: &M) -> M {
    [a[0] * b[0] + a[1] * b[2], a[0] * b[1] + a[1] * b[3],
     a[2] * b[0] + a[3] * b[2], a[2] * b[1] + a[3] * b[3],
     a[4] * b[0] + a[5] * b[2] + b[4], a[4] * b[1] + a[5] * b[3] + b[5]]
}
fn apply(m: &M, x: f64, y: f64) -> (f64, f64) { (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) }
fn translate(tx: f64, ty: f64) -> M { [1.0, 0.0, 0.0, 1.0, tx, ty] }

/// Word and line rules, as fractions of the font size. Fixed; tuned on dev only, then frozen.
pub const WORD_GAP: f64 = 0.15;      // a gap along the baseline wider than this starts a new word
pub const BACKSTEP: f64 = 0.5;      // moving back further than this starts a new word (and a new line)
pub const BASELINE_SHIFT: f64 = 0.5; // a baseline moving more than this starts a new word and line

/// One drawn glyph. Boxes are in points from the top-left of the visible page (after CropBox and /Rotate).
#[derive(Clone, Debug)]
pub struct Glyph {
    /// The Unicode text the file gives for this glyph; empty when it gives none.
    pub text: String,
    pub mapped: bool,
    /// The character code as drawn.
    pub code: u32,
    /// Index into `Doc::fonts`, or u32::MAX when no font was set.
    pub font: u32,
    /// Drawn with render mode 3 or 7 (neither filled nor stroked), as OCR layers are.
    pub invisible: bool,
    /// Painted only in white (the fill for render modes 0 and 4, the stroke for 1 and 5, both for 2
    /// and 6), so it shows nothing on a white page.
    pub white: bool,
    /// Drawn by an annotation's appearance stream, not the page's content.
    pub annot: bool,
    /// The box's centre is outside the visible page.
    pub offpage: bool,
    /// The clip in force when it was drawn cut part of it (the box is then the visible part), or
    /// hid all of it (the box is then the drawn box).
    pub clipped: bool,
    pub hidden: bool,
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    /// The box with the glyph's outline added, where the outline reaches past it (D91): italic overhang,
    /// capitals above the font's /Ascent. Clipped like the box. None when the outline is unknown or inside.
    pub ink: Option<[f64; 4]>,
    /// Baseline start, in page coordinates.
    pub ox: f64, pub oy: f64,
    /// Font size on the page, in points.
    pub size: f64,
    /// Drawing order on the page: glyphs and images share one count, so a higher number is drawn later (on top).
    pub order: u32,
    // in default user space, for grouping: baseline start and end, unit direction, and the glyph's quad
    ux: f64, uy: f64, ex: f64, ey: f64, dx: f64, dy: f64, quad: [(f64, f64); 4],
    ink_quad: Option<[(f64, f64); 4]>,
    clip: Option<[f64; 4]>,
}

pub struct Word {
    pub text: String,
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    pub line: usize,
    pub font: u32,
    pub size: f64,
    pub unmapped: usize,
    pub invisible: bool, pub annot: bool, pub offpage: bool,
    /// Every glyph of it is painted only in white.
    pub white: bool,
    /// Every glyph of it is hidden by its clip.
    pub hidden: bool,
    /// Index of the word's first glyph in `Page::glyphs`, and how many glyphs it has.
    pub first: usize, pub count: usize,
    /// Drawing order of its first-drawn glyph.
    pub order: u32,
}

pub struct Page { pub n: usize, pub width: f64, pub height: f64, pub rotate: i64, pub glyphs: Vec<Glyph>, pub words: Vec<Word>, pub verdict: &'static str, pub images: Vec<Image>, pub regions: Vec<Region>, pub lines: Vec<Line>, pub paths: Vec<Path>, pub vectors: Vec<Vector>, pub annots: Vec<Annot>, pub map: Vec<Entry> }

// regions: images

/// One drawn piece of an image: its object number (0 inline), its box on the page, and which way
/// its stored pixels face there.
#[derive(Clone, Debug)]
pub struct Piece { pub obj: u32, pub bx: [f64; 4], pub turn: pixels::Turn }

/// One image drawn on the page, or several that tile one picture (strips), merged.
/// The box is in points from the top-left of the visible page, clipped to it.
#[derive(Clone, Debug)]
pub struct Image {
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    /// Pixel size as stored (summed along the joined side for merged strips).
    pub px_w: u32, pub px_h: u32,
    /// Effective resolution on the page, pixels per inch, along the image's own width and height.
    pub dpi_x: f64, pub dpi_y: f64,
    /// A stencil mask (/ImageMask true): one colour painted through a 1-bit shape.
    pub mask: bool,
    pub inline: bool,
    pub annot: bool,
    /// Object number of the image XObject (0 for inline images); the first one for merged strips.
    pub obj: u32,
    /// How many drawn images were merged into this one (1 when it wasn't).
    pub parts: u32,
    /// Each drawn piece, for judging a merged image by all its strips (kind layer, 8g). Not in the JSON.
    pub pieces: Vec<Piece>,
    /// An inline image's dictionary (between BI and ID) and its data (between ID and EI), for its
    /// pixels (pixels::inline_thumbnail); None for an XObject or a merged image. Not in the JSON.
    pub inline_src: Option<std::sync::Arc<(Vec<u8>, Vec<u8>)>>,
    /// Clipping to the page cut part of the drawn box.
    pub clipped: bool,
    /// Axis-aligned on the page (the image's unit square isn't rotated or skewed).
    pub upright: bool,
    /// Nothing of it is on the visible page; the box is then the drawn box, unclipped.
    pub offpage: bool,
    /// The clip it was drawn with hides all of it; the box is then its part on the page.
    pub hidden: bool,
    /// Drawing order (the first part's, for merged strips).
    pub order: u32,
}

/// One annotation on the page: its /Rect on the page, what it is, and whether it draws anything.
/// Every annotation is kept, hidden ones and ones with no appearance included, flagged.
#[derive(Clone, Debug)]
pub struct Annot {
    pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64,
    /// /Subtype, such as "Widget", "Link", "Text", "Stamp", "Popup" (empty when missing).
    pub subtype: String,
    /// For a form field (a widget), its /FT, from the widget or its parent field: "Tx", "Btn", "Ch", "Sig".
    pub field: String,
    /// /F has Hidden (2) or NoView (32) set: a viewer doesn't show it.
    pub hidden: bool,
    /// It has a normal appearance stream that was drawn (its marks are in the map, flagged annot).
    pub appearance: bool,
    pub offpage: bool,
    pub order: u32,
}

#[derive(Clone)]
struct RawAnnot { rect: [f64; 4], subtype: String, field: String, hidden: bool, appearance: bool, order: u32 }

/// An image as drawn, before placement: its matrix (unit square to user space) and its pixels.
#[derive(Clone)]
struct Drawn { ctm: M, px_w: u32, px_h: u32, mask: bool, inline: bool, annot: bool, obj: u32, order: u32, clip: Option<[f64; 4]>, src: Option<std::sync::Arc<(Vec<u8>, Vec<u8>)>> }

/// Strips are joined when their edges across the join agree within STRIP_EDGE points and they
/// touch along it within STRIP_GAP points.
pub const STRIP_EDGE: f64 = 0.5;
pub const STRIP_GAP: f64 = 1.0;

/// Share of glyphs a verdict needs: half of the visible glyphs invisible makes an OCR layer, and half
/// of the non-space glyphs undecodable makes a garbled text layer.
pub const VERDICT_SHARE: f64 = 0.5;

/// A glyph whose text doesn't decode to real characters: unmapped, U+FFFD, private use, or control.
fn undecodable(g: &Glyph) -> bool {
    !g.mapped || g.text.chars().any(|c| {
        let u = c as u32;
        c == '\u{fffd}' || (0xE000..=0xF8FF).contains(&u) || u >= 0xF0000 || (u < 0x20 && !matches!(c, '\t' | '\n' | '\r')) || (0x7F..0xA0).contains(&u)
    })
}

/// The page verdict, by fixed rules in this order: none, invisible, garbled, text.
fn verdict(glyphs: &[Glyph]) -> &'static str {
    let shown: Vec<&Glyph> = glyphs.iter().filter(|g| !g.offpage && !g.hidden).collect();
    if shown.is_empty() { return "none"; }
    if shown.iter().filter(|g| g.invisible).count() as f64 >= VERDICT_SHARE * shown.len() as f64 { return "invisible"; }
    let ink: Vec<&&Glyph> = shown.iter().filter(|g| !is_space(g)).collect();
    if ink.is_empty() { return "none"; }
    if ink.iter().filter(|g| undecodable(g)).count() as f64 >= VERDICT_SHARE * ink.len() as f64 { return "garbled"; }
    "text"
}

pub struct FontInfo { pub base: String, pub kind: Kind, pub encoding: String, pub to_unicode: bool, pub embedded: bool, pub widths: &'static str }

pub struct Doc {
    /// "ok", "not_pdf", or "encrypted" (a password we can't open, or an unsupported handler).
    pub status: &'static str,
    pub pages: Vec<Page>,
    pub fonts: Vec<FontInfo>,
}

#[derive(Clone)]
struct GState {
    ctm: M, font: Option<u32>, size: f64, tc: f64, tw: f64, tz: f64, tl: f64, ts: f64, tr: i64,
    // paths: line width, the clip's box in default user space (None: the whole page), whether the
    // fill and stroke colours are white, and the colour spaces' kinds for sc and scn (see cs_kind)
    lw: f64, cap: i64, clip: Option<[f64; 4]>, fill_white: bool, stroke_white: bool, fill_cs: u8, stroke_cs: u8,
}

impl Default for GState {
    fn default() -> Self {
        GState { ctm: IDENT, font: None, size: 0.0, tc: 0.0, tw: 0.0, tz: 100.0, tl: 0.0, ts: 0.0, tr: 0,
            lw: 1.0, cap: 0, clip: None, fill_white: false, stroke_white: false, fill_cs: 1, stroke_cs: 1 }
    }
}

/// A painted path or shading as drawn: its box in default user space (None: the whole clip, or
/// the page when there's no clip), before placement on the page.
#[derive(Clone)]
struct RawPath { b: Option<[f64; 4]>, clip: Option<[f64; 4]>, order: u32, fill: bool, stroke: bool, shading: bool, white: bool, annot: bool, dot: bool, empty: bool, lines: u32, curves: u32, rect: bool, closed: bool, seg_rows: u32, seg_cols: u32 }

fn grow(b: &mut Option<[f64; 4]>, x: f64, y: f64) {
    match b {
        Some(v) => { v[0] = v[0].min(x); v[1] = v[1].min(y); v[2] = v[2].max(x); v[3] = v[3].max(y); }
        None => *b = Some([x, y, x, y]),
    }
}

fn intersect(a: Option<[f64; 4]>, b: [f64; 4]) -> [f64; 4] {
    match a {
        Some(a) => [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]).max(a[0].max(b[0])), a[3].min(b[3]).max(a[1].max(b[1]))],
        None => b,
    }
}

/// Fonts are loaded once per object and shared by every page that uses them.
struct Fonts { by_obj: HashMap<u32, u32>, list: Vec<font::Font> }

impl Fonts {
    fn lookup(&mut self, pdf: &Pdf, resources: Option<&[u8]>, name: &[u8]) -> Option<u32> {
        let fonts = resources.and_then(|r| get(r, b"/Font")).and_then(|v| pdf.resolve(&v))?;
        let mut key = Vec::with_capacity(name.len() + 1);
        key.push(b'/');
        key.extend_from_slice(name);
        match get(&fonts, &key)? {
            Val::Ref(n) => {
                if let Some(&i) = self.by_obj.get(&n) { return Some(i); }
                let d = pdf.dict(n)?;
                self.list.push(font::Font::load(pdf, &d));
                let i = (self.list.len() - 1) as u32;
                self.by_obj.insert(n, i);
                Some(i)
            }
            Val::Dict(d) => { self.list.push(font::Font::load(pdf, &d)); Some((self.list.len() - 1) as u32) }
            _ => None,
        }
    }
}

enum Tok { Num(f64), Str(Vec<u8>), Name(Vec<u8>), Arr(Vec<Tok>), Other }

/// One operand starting at `i` (after whitespace), or None when an operator starts there.
fn operand(s: &[u8], i: &mut usize) -> Option<Tok> {
    let c = s[*i];
    match c {
        b'(' => { let e = skip_string(s, *i); let v = string_at(s, *i).unwrap_or_default(); *i = e; Some(Tok::Str(v)) }
        b'<' if s.get(*i + 1) == Some(&b'<') => { *i = matching(s, *i); Some(Tok::Other) }
        b'<' => {
            let e = find(s, b">", *i).unwrap_or(s.len());
            let v = string_at(s, *i).unwrap_or_default();
            *i = (e + 1).min(s.len());
            Some(Tok::Str(v))
        }
        b'[' => {
            *i += 1;
            let mut items = Vec::new();
            loop {
                *i = skip_ws(s, *i);
                if *i >= s.len() { break; }
                if s[*i] == b']' { *i += 1; break; }
                match operand(s, i) {
                    Some(t) => items.push(t),
                    None => { let mut j = *i; while j < s.len() && is_regular(s[j]) { j += 1; } *i = j.max(*i + 1); }
                }
            }
            Some(Tok::Arr(items))
        }
        b'/' => {
            let mut j = *i + 1;
            while j < s.len() && is_regular(s[j]) { j += 1; }
            let v = s[*i + 1..j].to_vec();
            *i = j;
            Some(Tok::Name(v))
        }
        b'0'..=b'9' | b'-' | b'+' | b'.' => {
            let mut j = *i + 1;
            while j < s.len() && (s[j].is_ascii_digit() || s[j] == b'.') { j += 1; }
            let v = std::str::from_utf8(&s[*i..j]).ok().and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0);
            *i = j;
            Some(Tok::Num(v))
        }
        _ => None,
    }
}

fn num(t: &Tok) -> f64 { if let Tok::Num(v) = t { *v } else { 0.0 } }

fn matrix_of(pdf: &Pdf, d: &[u8]) -> M {
    match get(d, b"/Matrix").map(|v| pdf.direct(v)) {
        Some(Val::Array(a)) => { let v = nums_in(&a); if v.len() == 6 { [v[0], v[1], v[2], v[3], v[4], v[5]] } else { IDENT } }
        _ => IDENT,
    }
}

struct Run<'p, 'a> { pdf: &'p Pdf<'a>, fonts: Fonts, out: Vec<Glyph>, annot: bool, drawn: Vec<Drawn>, paths: Vec<RawPath>, annots: Vec<RawAnnot>, seq: u32 }

impl<'p, 'a> Run<'p, 'a> {
    /// Show a string: one glyph per character code, each placed by the text rendering matrix
    /// (ISO 32000-1 9.4.4), then the text matrix advanced by the glyph's width.
    fn show(&mut self, g: &GState, tm: &mut M, bytes: &[u8]) {
        let invisible = g.tr == 3 || g.tr == 7;
        let white = match g.tr { 0 | 4 => g.fill_white, 1 | 5 => g.stroke_white, 2 | 6 => g.fill_white && g.stroke_white, _ => false };
        let th = g.tz / 100.0;
        let fid = match g.font { Some(f) => f, None => {
            // text shown before any font was set: it can't be decoded or measured
            for &b in bytes {
                let (x, y) = apply(&mul(tm, &g.ctm), 0.0, 0.0);
                self.out.push(blank(b as u32, invisible, white, self.annot, x, y, self.seq, g.clip));
                self.seq += 1;
            }
            return;
        } };
        let f = &self.fonts.list[fid as usize];
        let (desc, asc) = f.vertical();
        for (code, len) in f.codes(bytes) {
            let w0 = f.advance(code);
            let trm = mul(&mul(&[g.size * th, 0.0, 0.0, g.size, 0.0, g.ts], tm), &g.ctm);
            // the advance box, grown to a Type3 glyph's own box where it reaches further
            let (mut bx0, mut by0, mut bx1, mut by1) = (0.0f64, desc, w0, asc);
            if let Some(b) = f.glyph_box(code) { bx0 = bx0.min(b[0]); by0 = by0.min(b[1]); bx1 = bx1.max(b[2]); by1 = by1.max(b[3]); }
            let quad = [apply(&trm, bx0, by0), apply(&trm, bx1, by0), apply(&trm, bx1, by1), apply(&trm, bx0, by1)];
            // the outline, where it reaches past that box
            let ink_quad = f.outline_box(code).filter(|b| b[0] < bx0 || b[1] < by0 || b[2] > bx1 || b[3] > by1).map(|b| {
                let (ix0, iy0, ix1, iy1) = (bx0.min(b[0]), by0.min(b[1]), bx1.max(b[2]), by1.max(b[3]));
                [apply(&trm, ix0, iy0), apply(&trm, ix1, iy0), apply(&trm, ix1, iy1), apply(&trm, ix0, iy1)]
            });
            let (ux, uy) = apply(&trm, 0.0, 0.0);
            let (ex, ey) = apply(&trm, w0, 0.0);
            let dl = (trm[0] * trm[0] + trm[1] * trm[1]).sqrt();
            let (dx, dy) = if dl > 1e-9 { (trm[0] / dl, trm[1] / dl) } else { (1.0, 0.0) };
            let size = (trm[2] * trm[2] + trm[3] * trm[3]).sqrt();
            let t = f.unicode(code);
            self.out.push(Glyph {
                mapped: t.is_some(), text: t.unwrap_or_default(), code, font: fid, invisible, white, annot: self.annot, offpage: false, clipped: false, hidden: false,
                x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0, ink: None, ox: 0.0, oy: 0.0, size, order: self.seq, ux, uy, ex, ey, dx, dy, quad, ink_quad, clip: g.clip,
            });
            self.seq += 1;
            let mut tx = w0 * g.size + g.tc;
            if f.is_word_space(code, len) { tx += g.tw; }
            *tm = mul(&translate(tx * th, 0.0), tm);
        }
    }

    fn run(&mut self, content: &[u8], resources: Option<&[u8]>, gs0: GState, depth: usize) {
        let pdf = self.pdf;
        let xobjects = resources.and_then(|r| get(r, b"/XObject")).and_then(|v| pdf.resolve(&v));
        let mut g = gs0;
        let mut stack: Vec<GState> = Vec::new();
        let mut ops: Vec<Tok> = Vec::new();
        let (mut tm, mut tlm) = (IDENT, IDENT);
        // the path being built: its box in default user space, the current and start points in user space
        let mut pbox: Option<[f64; 4]> = None;
        let (mut cur, mut start) = ((0.0, 0.0), (0.0, 0.0));
        // the same path as segments on the page's user space, by subpath, for the stroke's box
        let mut subs: Vec<(Vec<vector::Seg>, bool, (f64, f64))> = Vec::new();
        // the path under construction used the re operator (a vector kind measure)
        let mut had_re = false;
        let mut clip_next = false;
        let s = content;
        let mut i = 0;
        while i < s.len() {
            if is_ws(s[i]) { i += 1; continue; }
            if s[i] == b'%' { while i < s.len() && s[i] != b'\n' && s[i] != b'\r' { i += 1; } continue; }
            if let Some(t) = operand(s, &mut i) { ops.push(t); continue; }
            let mut j = i;
            while j < s.len() && is_regular(s[j]) { j += 1; }
            if j == i { i += 1; continue; }
            let op = &s[i..j];
            i = j;
            let n = ops.len();
            match op {
                b"true" | b"false" | b"null" => { ops.push(Tok::Other); continue; }
                b"q" => stack.push(g.clone()),
                b"Q" => { if let Some(x) = stack.pop() { g = x; } }
                b"cm" if n >= 6 => {
                    let m = [num(&ops[n - 6]), num(&ops[n - 5]), num(&ops[n - 4]), num(&ops[n - 3]), num(&ops[n - 2]), num(&ops[n - 1])];
                    g.ctm = mul(&m, &g.ctm);
                }
                b"BT" => { tm = IDENT; tlm = IDENT; }
                // paths (vector.rs): build the box as the path is built, record it when it's painted
                b"m" if n >= 2 => {
                    cur = (num(&ops[n - 2]), num(&ops[n - 1])); start = cur;
                    let (x, y) = apply(&g.ctm, cur.0, cur.1); grow(&mut pbox, x, y);
                    subs.push((Vec::new(), false, (x, y)));
                }
                b"l" if n >= 2 => {
                    let p = apply(&g.ctm, cur.0, cur.1);
                    cur = (num(&ops[n - 2]), num(&ops[n - 1]));
                    let (x, y) = apply(&g.ctm, cur.0, cur.1); grow(&mut pbox, x, y);
                    if subs.is_empty() { subs.push((Vec::new(), false, p)); }
                    subs.last_mut().unwrap().0.push(vector::Seg::Line(p, (x, y)));
                }
                b"c" | b"v" | b"y" if n >= 4 => {
                    let v: Vec<f64> = ops[n.saturating_sub(6)..].iter().map(num).collect();
                    let (p1, p2, p3) = match op {
                        b"c" if n >= 6 => ((v[0], v[1]), (v[2], v[3]), (v[4], v[5])),
                        b"v" => (cur, (v[v.len() - 4], v[v.len() - 3]), (v[v.len() - 2], v[v.len() - 1])),
                        b"y" => ((v[v.len() - 4], v[v.len() - 3]), (v[v.len() - 2], v[v.len() - 1]), (v[v.len() - 2], v[v.len() - 1])),
                        _ => { ops.clear(); continue; }
                    };
                    let q: Vec<(f64, f64)> = [cur, p1, p2, p3].iter().map(|p| apply(&g.ctm, p.0, p.1)).collect();
                    let (x0, x1) = vector::cubic_range(q[0].0, q[1].0, q[2].0, q[3].0);
                    let (y0, y1) = vector::cubic_range(q[0].1, q[1].1, q[2].1, q[3].1);
                    grow(&mut pbox, x0, y0); grow(&mut pbox, x1, y1);
                    if subs.is_empty() { subs.push((Vec::new(), false, q[0])); }
                    subs.last_mut().unwrap().0.push(vector::Seg::Curve([x0, y0, x1, y1], q[0], q[3]));
                    cur = p3;
                }
                b"h" => {
                    // close: a line back to the start, and the subpath is closed (a join, not caps)
                    if let Some(last) = subs.last_mut() {
                        let (p, q) = (apply(&g.ctm, cur.0, cur.1), apply(&g.ctm, start.0, start.1));
                        if (p.0 - q.0).abs() > 1e-9 || (p.1 - q.1).abs() > 1e-9 { last.0.push(vector::Seg::Line(p, q)); }
                        last.1 = true;
                    }
                    cur = start;
                }
                b"re" if n >= 4 => {
                    let (x, y, w, h) = (num(&ops[n - 4]), num(&ops[n - 3]), num(&ops[n - 2]), num(&ops[n - 1]));
                    let q: Vec<(f64, f64)> = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].iter().map(|p| apply(&g.ctm, p.0, p.1)).collect();
                    for &(a, b) in &q { grow(&mut pbox, a, b); }
                    subs.push(((0..4).map(|k| vector::Seg::Line(q[k], q[(k + 1) % 4])).collect(), true, q[0]));
                    had_re = true;
                    cur = (x, y); start = cur;
                }
                b"W" | b"W*" => clip_next = true,
                b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n" => {
                    let stroke = matches!(op, b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*");
                    let fill = matches!(op, b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*");
                    if let Some(b) = pbox {
                        if stroke || fill {
                            let mut v = if fill { Some(b) } else { None };
                            // all its points one point: a round-capped stroke paints a dot, a fill one device pixel, else nothing
                            let point = b[2] - b[0] < 1e-9 && b[3] - b[1] < 1e-9;
                            let mut dot = false;
                            if stroke {
                                // half the line width on the page; a zero width still paints a hairline
                                let hw = (g.lw.max(0.0) * (g.ctm[0] * g.ctm[3] - g.ctm[1] * g.ctm[2]).abs().sqrt()).max(0.5) / 2.0;
                                if let Some(sb) = vector::stroke_box(&subs, hw, g.cap) {
                                    dot = point;
                                    v = Some(match v { Some(f) => [f[0].min(sb[0]), f[1].min(sb[1]), f[2].max(sb[2]), f[3].max(sb[3])], None => sb });
                                }
                            }
                            let v = v.unwrap_or(b);
                            let white = (!fill || g.fill_white) && (!stroke || g.stroke_white);
                            let lines = subs.iter().map(|s| s.0.iter().filter(|x| matches!(x, vector::Seg::Line(..))).count() as u32).sum();
                            let curves = subs.iter().map(|s| s.0.iter().filter(|x| matches!(x, vector::Seg::Curve(..))).count() as u32).sum();
                            let closed = subs.iter().any(|s| s.1);
                            let (seg_rows, seg_cols) = vector::seg_grid(&subs, &b);
                            self.paths.push(RawPath { b: Some(v), clip: g.clip, order: self.seq, fill, stroke, shading: false, white, annot: self.annot, dot, empty: point && !dot && !fill, lines, curves, rect: had_re, closed, seg_rows, seg_cols });
                            self.seq += 1;
                        }
                        if clip_next { g.clip = Some(intersect(g.clip, b)); }
                    }
                    pbox = None;
                    subs.clear();
                    had_re = false;
                    clip_next = false;
                }
                b"sh" => {
                    self.paths.push(RawPath { b: None, clip: g.clip, order: self.seq, fill: true, stroke: false, shading: true, white: false, annot: self.annot, dot: false, empty: false, lines: 0, curves: 0, rect: false, closed: false, seg_rows: 0, seg_cols: 0 });
                    self.seq += 1;
                }
                b"w" if n >= 1 => g.lw = num(&ops[n - 1]),
                b"J" if n >= 1 => g.cap = num(&ops[n - 1]) as i64,
                b"g" if n >= 1 => { g.fill_cs = 1; g.fill_white = is_white(1, &ops); }
                b"G" if n >= 1 => { g.stroke_cs = 1; g.stroke_white = is_white(1, &ops); }
                b"rg" if n >= 3 => { g.fill_cs = 3; g.fill_white = is_white(3, &ops); }
                b"RG" if n >= 3 => { g.stroke_cs = 3; g.stroke_white = is_white(3, &ops); }
                b"k" if n >= 4 => { g.fill_cs = 4; g.fill_white = is_white(4, &ops); }
                b"K" if n >= 4 => { g.stroke_cs = 4; g.stroke_white = is_white(4, &ops); }
                // a new colour space starts at its initial colour, which is never white here (black for
                // gray, RGB and CMYK); sc and scn are read by the space's kind (ICCBased, CalRGB and so on)
                b"cs" | b"CS" => {
                    let kind = match ops.last() { Some(Tok::Name(nm)) => cs_kind(pdf, resources, nm), _ => 0 };
                    if op == b"cs" { g.fill_cs = kind; g.fill_white = false; } else { g.stroke_cs = kind; g.stroke_white = false; }
                }
                b"sc" | b"scn" => g.fill_white = is_white(g.fill_cs, &ops),
                b"SC" | b"SCN" => g.stroke_white = is_white(g.stroke_cs, &ops),
                b"Tf" if n >= 2 => {
                    if let Tok::Name(nm) = &ops[n - 2] { g.font = self.fonts.lookup(pdf, resources, nm); }
                    g.size = num(&ops[n - 1]);
                }
                b"Tc" if n >= 1 => g.tc = num(&ops[n - 1]),
                b"Tw" if n >= 1 => g.tw = num(&ops[n - 1]),
                b"Tz" if n >= 1 => g.tz = num(&ops[n - 1]),
                b"TL" if n >= 1 => g.tl = num(&ops[n - 1]),
                b"Ts" if n >= 1 => g.ts = num(&ops[n - 1]),
                b"Tr" if n >= 1 => g.tr = num(&ops[n - 1]) as i64,
                b"Td" | b"TD" if n >= 2 => {
                    let (tx, ty) = (num(&ops[n - 2]), num(&ops[n - 1]));
                    if op == b"TD" { g.tl = -ty; }
                    tlm = mul(&translate(tx, ty), &tlm);
                    tm = tlm;
                }
                b"Tm" if n >= 6 => {
                    tlm = [num(&ops[n - 6]), num(&ops[n - 5]), num(&ops[n - 4]), num(&ops[n - 3]), num(&ops[n - 2]), num(&ops[n - 1])];
                    tm = tlm;
                }
                b"T*" => { tlm = mul(&translate(0.0, -g.tl), &tlm); tm = tlm; }
                b"Tj" if n >= 1 => { if let Tok::Str(b) = &ops[n - 1] { self.show(&g, &mut tm, b); } }
                b"'" if n >= 1 => {
                    tlm = mul(&translate(0.0, -g.tl), &tlm); tm = tlm;
                    if let Tok::Str(b) = &ops[n - 1] { self.show(&g, &mut tm, b); }
                }
                b"\"" if n >= 3 => {
                    g.tw = num(&ops[n - 3]); g.tc = num(&ops[n - 2]);
                    tlm = mul(&translate(0.0, -g.tl), &tlm); tm = tlm;
                    if let Tok::Str(b) = &ops[n - 1] { self.show(&g, &mut tm, b); }
                }
                b"TJ" if n >= 1 => {
                    if let Tok::Arr(items) = &ops[n - 1] {
                        for it in items {
                            match it {
                                Tok::Str(b) => self.show(&g, &mut tm, b),
                                // a number moves the pen back by thousandths of the font size
                                Tok::Num(v) => tm = mul(&translate(-v / 1000.0 * g.size * g.tz / 100.0, 0.0), &tm),
                                _ => {}
                            }
                        }
                    }
                }
                b"Do" if n >= 1 => {
                    if let (Tok::Name(nm), Some(x)) = (&ops[n - 1], &xobjects) { self.form(x, nm, &g, resources, depth); }
                }
                b"BI" => {
                    // inline image: skip its data up to a whitespace-delimited EI
                    let id = find(s, b"ID", i).unwrap_or(s.len());
                    let mut k = id + 2;
                    // regions: ASCII-encoded data can hold "EI" itself, so skip to its end marker first
                    if let Some(end) = ascii_data_end(&s[i..id.min(s.len())]) {
                        if let Some(e) = find(s, end, k) { k = e + end.len(); }
                    }
                    let data_end = loop {
                        match find(s, b"EI", k) {
                            Some(e) if (e == 0 || is_ws(s[e - 1])) && (e + 2 >= s.len() || !is_regular(s[e + 2])) => { k = e + 2; break e; }
                            Some(e) => k = e + 2,
                            None => { k = s.len(); break s.len(); }
                        }
                    };
                    // regions: its size comes from the dictionary between BI and ID, its pixels from the data
                    // between ID and EI (one white-space byte after ID and one before EI aren't data)
                    let (dict, mut a, mut b) = (&s[i..id.min(s.len())], (id + 2).min(s.len()), data_end.max(id + 2).min(s.len()));
                    if a < b && is_ws(s[a]) { a += 1; }
                    if b > a && is_ws(s[b - 1]) { b -= 1; }
                    self.inline_image(dict, &s[a..b.max(a)], &g);
                    i = k;
                }
                _ => {}
            }
            ops.clear();
        }
    }

    fn form(&mut self, xobjects: &[u8], name: &[u8], g: &GState, resources: Option<&[u8]>, depth: usize) {
        if depth >= MAX_FORM_DEPTH { return; }
        let pdf = self.pdf;
        let mut key = Vec::with_capacity(name.len() + 1);
        key.push(b'/');
        key.extend_from_slice(name);
        let n = match get(xobjects, &key) { Some(Val::Ref(n)) => n, _ => return };
        let d = match pdf.dict(n) { Some(d) => d, None => return };
        // regions: an image XObject is recorded with the matrix it's drawn with
        if matches!(get(&d, b"/Subtype"), Some(Val::Name(s)) if s == b"Image") {
            let int = |k: &[u8]| match get(&d, k).map(|v| pdf.direct(v)) { Some(Val::Num(x)) if x > 0.0 => x as u32, _ => 0 };
            let mask = flag(&d, b"/ImageMask");
            self.drawn.push(Drawn { ctm: g.ctm, px_w: int(b"/Width"), px_h: int(b"/Height"), mask, inline: false, annot: self.annot, obj: n, order: self.seq, clip: g.clip, src: None });
            self.seq += 1;
            return;
        }
        if !matches!(get(&d, b"/Subtype"), Some(Val::Name(s)) if s == b"Form") { return; }
        let m = matrix_of(pdf, &d);
        let res = get(&d, b"/Resources").and_then(|v| pdf.resolve(&v));
        if let Some(body) = pdf.stream(n) {
            let mut gf = g.clone();
            gf.ctm = mul(&m, &g.ctm);
            self.run(&body, res.as_deref().or(resources), gf, depth + 1);
        }
    }

    /// regions: an inline image, from its dictionary (the bytes between BI and ID), with the
    /// abbreviated keys or the full ones.
    fn inline_image(&mut self, dict: &[u8], data: &[u8], g: &GState) {
        let mut d = Vec::with_capacity(dict.len() + 4);
        d.extend_from_slice(b"<<");
        d.extend_from_slice(dict);
        d.extend_from_slice(b">>");
        let int = |a: &[u8], b: &[u8]| match get(&d, a).or_else(|| get(&d, b)) { Some(Val::Num(x)) if x > 0.0 => x as u32, _ => 0 };
        let mask = flag(&d, b"/IM") || flag(&d, b"/ImageMask");
        self.drawn.push(Drawn { ctm: g.ctm, px_w: int(b"/W", b"/Width"), px_h: int(b"/H", b"/Height"), mask, inline: true, annot: self.annot, obj: 0, order: self.seq, clip: g.clip,
                                src: Some(std::sync::Arc::new((dict.to_vec(), data.to_vec()))) });
        self.seq += 1;
    }

    /// Text drawn by the page's annotations: each shown annotation's normal appearance stream, placed
    /// on its /Rect (ISO 32000-1 12.5.5). Hidden and NoView annotations are skipped. A widget with a
    /// value but no appearance stream draws nothing, so it contributes nothing here.
    fn annotations(&mut self, page_dict: &[u8]) {
        let pdf = self.pdf;
        let list = match get(page_dict, b"/Annots").map(|v| pdf.direct(v)) { Some(Val::Array(a)) => refs_in(&a), _ => return };
        for r in list {
            let Some(a) = pdf.dict(r) else { continue };
            let flags = match get(&a, b"/F").map(|v| pdf.direct(v)) { Some(Val::Num(f)) => f as u32, _ => 0 };
            // map: every annotation is a region, whether or not it draws
            let rect = nums_in(&match get(&a, b"/Rect").map(|v| pdf.direct(v)) { Some(Val::Array(b)) => b, _ => Vec::new() });
            if rect.len() == 4 {
                let name = |v: Option<Val>| match v { Some(Val::Name(s)) => String::from_utf8_lossy(&s).into_owned(), _ => String::new() };
                let subtype = name(get(&a, b"/Subtype"));
                // /FT is inherited from the parent field (ISO 32000-1 12.7.3.1); a few levels are enough
                let mut field = name(get(&a, b"/FT"));
                let mut at = a.clone();
                for _ in 0..8 {
                    if !field.is_empty() { break; }
                    let Some(Val::Ref(pr)) = get(&at, b"/Parent") else { break };
                    let Some(pd) = pdf.dict(pr) else { break };
                    field = name(get(&pd, b"/FT"));
                    at = pd;
                }
                self.annots.push(RawAnnot {
                    rect: [rect[0].min(rect[2]), rect[1].min(rect[3]), rect[0].max(rect[2]), rect[1].max(rect[3])],
                    subtype, field, hidden: flags & (2 | 32) != 0, appearance: false, order: self.seq,
                });
                self.seq += 1;
            }
            let k = self.annots.len();
            if flags & (2 | 32) != 0 { continue; }
            if matches!(get(&a, b"/Subtype"), Some(Val::Name(s)) if s == b"Popup") { continue; }
            let Some(ap) = get(&a, b"/AP").and_then(|v| pdf.resolve(&v)) else { continue };
            // /N is a stream, or a dictionary of appearance states chosen by /AS
            let n = match get(&ap, b"/N") {
                Some(Val::Ref(n)) if pdf.dict(n).map(|d| get(&d, b"/BBox").is_some()).unwrap_or(false) => n,
                Some(v) => {
                    let states = match pdf.resolve(&v) { Some(s) => s, None => continue };
                    let state = match get(&a, b"/AS") { Some(Val::Name(s)) => s, _ => continue };
                    let mut key = vec![b'/'];
                    key.extend_from_slice(&state);
                    match get(&states, &key) { Some(Val::Ref(n)) => n, _ => continue }
                }
                None => continue,
            };
            let Some(fd) = pdf.dict(n) else { continue };
            let bbox = nums_in(&match get(&fd, b"/BBox").map(|v| pdf.direct(v)) { Some(Val::Array(b)) => b, _ => continue });
            let rect = nums_in(&match get(&a, b"/Rect").map(|v| pdf.direct(v)) { Some(Val::Array(b)) => b, _ => continue });
            if bbox.len() != 4 || rect.len() != 4 { continue; }
            let m = matrix_of(pdf, &fd);
            let pts = [apply(&m, bbox[0], bbox[1]), apply(&m, bbox[2], bbox[1]), apply(&m, bbox[2], bbox[3]), apply(&m, bbox[0], bbox[3])];
            let (bx0, bx1) = (pts.iter().map(|p| p.0).fold(f64::MAX, f64::min), pts.iter().map(|p| p.0).fold(f64::MIN, f64::max));
            let (by0, by1) = (pts.iter().map(|p| p.1).fold(f64::MAX, f64::min), pts.iter().map(|p| p.1).fold(f64::MIN, f64::max));
            let (rx0, ry0, rx1, ry1) = (rect[0].min(rect[2]), rect[1].min(rect[3]), rect[0].max(rect[2]), rect[1].max(rect[3]));
            if bx1 - bx0 < 1e-6 || by1 - by0 < 1e-6 { continue; }
            let (sx, sy) = ((rx1 - rx0) / (bx1 - bx0), (ry1 - ry0) / (by1 - by0));
            let fit = [sx, 0.0, 0.0, sy, rx0 - bx0 * sx, ry0 - by0 * sy];
            let res = get(&fd, b"/Resources").and_then(|v| pdf.resolve(&v));
            if let Some(body) = pdf.stream(n) {
                let g = GState { ctm: mul(&m, &fit), ..GState::default() };
                self.annot = true;
                self.run(&body, res.as_deref(), g, 1);
                self.annot = false;
                if rect.len() == 4 && k > 0 { self.annots[k - 1].appearance = true; }
            }
        }
    }
}

fn blank(code: u32, invisible: bool, white: bool, annot: bool, x: f64, y: f64, order: u32, clip: Option<[f64; 4]>) -> Glyph {
    Glyph { text: String::new(), mapped: false, code, font: u32::MAX, invisible, white, annot, offpage: false, clipped: false, hidden: false,
            x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0, ink: None, ox: 0.0, oy: 0.0, size: 0.0, order,
            ux: x, uy: y, ex: x, ey: y, dx: 1.0, dy: 0.0, quad: [(x, y); 4], ink_quad: None, clip }
}

/// The visible page: CropBox (clipped to MediaBox) and /Rotate, as a map from default user space to
/// points from the page's top-left corner as displayed.
struct PageBox { x0: f64, y0: f64, x1: f64, y1: f64, rotate: i64 }

impl PageBox {
    fn of(pdf: &Pdf, page: u32) -> PageBox {
        let rect = |key: &[u8]| match pdf.inherited(page, key).map(|v| pdf.direct(v)) {
            Some(Val::Array(a)) => { let v = nums_in(&a); if v.len() == 4 { Some((v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3]))) } else { None } }
            _ => None,
        };
        let media = rect(b"/MediaBox").unwrap_or((0.0, 0.0, 612.0, 792.0));
        let crop = rect(b"/CropBox").map(|c| (c.0.max(media.0), c.1.max(media.1), c.2.min(media.2), c.3.min(media.3)))
            .filter(|c| c.2 > c.0 && c.3 > c.1).unwrap_or(media);
        let rotate = match pdf.inherited(page, b"/Rotate").map(|v| pdf.direct(v)) { Some(Val::Num(r)) => ((r as i64 % 360) + 360) % 360, _ => 0 };
        let rotate = if rotate % 90 == 0 { rotate } else { 0 };
        PageBox { x0: crop.0, y0: crop.1, x1: crop.2, y1: crop.3, rotate }
    }
    fn size(&self) -> (f64, f64) {
        let (w, h) = (self.x1 - self.x0, self.y1 - self.y0);
        if self.rotate % 180 == 0 { (w, h) } else { (h, w) }
    }
    /// User space -> displayed page, top-left origin (the page turned clockwise by /Rotate).
    fn map(&self, x: f64, y: f64) -> (f64, f64) {
        let (w, h) = (self.x1 - self.x0, self.y1 - self.y0);
        let (u, v) = (x - self.x0, self.y1 - y);
        match self.rotate { 90 => (h - v, u), 180 => (w - u, h - v), 270 => (v, w - u), _ => (u, v) }
    }
}

/// Each painted path through the page box, clipped to its clip and the page. A shading with no
/// clip paints the whole page.
fn place_paths(raw: &[RawPath], pb: &PageBox) -> Vec<Path> {
    let (w, h) = pb.size();
    let page = [0.0, 0.0, w, h];
    raw.iter().map(|r| {
        let clip = r.clip.map(|c| page_rect(pb, c));
        let drawn = r.b.map(|b| page_rect(pb, b)).unwrap_or(clip.unwrap_or(page));
        // off the page first, then hidden by its clip
        let (b, clipped, offpage, hidden) = match cut(drawn, page) {
            None => (drawn, true, true, false),
            Some((v, c1)) => match clip.map(|c| cut(v, c)) {
                None => (v, c1, false, false),
                Some(None) => (v, true, false, true),
                Some(Some((v2, c2))) => (v2, c1 || c2 || r.shading, false, false),
            },
        };
        Path { x0: b[0], y0: b[1], x1: b[2], y1: b[3], order: r.order, fill: r.fill, stroke: r.stroke, shading: r.shading,
            white: r.white, annot: r.annot, clipped, offpage, hidden, dot: r.dot, empty: r.empty, lines: r.lines, curves: r.curves, rect: r.rect, closed: r.closed, seg_rows: r.seg_rows, seg_cols: r.seg_cols }
    }).collect()
}

/// A box in default user space to one on the displayed page.
fn page_rect(pb: &PageBox, b: [f64; 4]) -> [f64; 4] {
    let pts = [pb.map(b[0], b[1]), pb.map(b[2], b[1]), pb.map(b[2], b[3]), pb.map(b[0], b[3])];
    [pts.iter().map(|p| p.0).fold(f64::MAX, f64::min), pts.iter().map(|p| p.1).fold(f64::MAX, f64::min),
     pts.iter().map(|p| p.0).fold(f64::MIN, f64::max), pts.iter().map(|p| p.1).fold(f64::MIN, f64::max)]
}

/// Cut a box on the page by a clip (already on the page): the visible part and whether anything
/// was cut, or None when nothing of it is inside. A box with no area (a glyph with no advance)
/// is inside when it lies within the clip's edges.
fn cut(b: [f64; 4], clip: [f64; 4]) -> Option<([f64; 4], bool)> {
    let v = [b[0].max(clip[0]), b[1].max(clip[1]), b[2].min(clip[2]), b[3].min(clip[3])];
    if v[2] < v[0] || v[3] < v[1] { return None; }
    if (v[2] - v[0]) * (v[3] - v[1]) <= 0.0 && (b[2] - b[0]) * (b[3] - b[1]) > 0.0 { return None; }
    let cutoff = v[0] > b[0] + 1e-6 || v[1] > b[1] + 1e-6 || v[2] < b[2] - 1e-6 || v[3] < b[3] - 1e-6;
    Some((v, cutoff))
}

fn place_annots(raw: &[RawAnnot], pb: &PageBox) -> Vec<Annot> {
    let (w, h) = pb.size();
    raw.iter().map(|r| {
        let b = page_rect(pb, r.rect);
        let (b, offpage) = match cut(b, [0.0, 0.0, w, h]) { Some((v, _)) => (v, false), None => (b, true) };
        Annot { x0: b[0], y0: b[1], x1: b[2], y1: b[3], subtype: r.subtype.clone(), field: r.field.clone(),
            hidden: r.hidden, appearance: r.appearance, offpage, order: r.order }
    }).collect()
}

fn place(glyphs: &mut [Glyph], pb: &PageBox) {
    let (w, h) = pb.size();
    for g in glyphs.iter_mut() {
        let pts: Vec<(f64, f64)> = g.quad.iter().map(|p| pb.map(p.0, p.1)).collect();
        g.x0 = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        g.x1 = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max);
        g.y0 = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
        g.y1 = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        let (ox, oy) = pb.map(g.ux, g.uy);
        g.ox = ox;
        g.oy = oy;
        g.ink = g.ink_quad.map(|q| {
            let pts: Vec<(f64, f64)> = q.iter().map(|p| pb.map(p.0, p.1)).collect();
            [pts.iter().map(|p| p.0).fold(f64::MAX, f64::min), pts.iter().map(|p| p.1).fold(f64::MAX, f64::min),
             pts.iter().map(|p| p.0).fold(f64::MIN, f64::max), pts.iter().map(|p| p.1).fold(f64::MIN, f64::max)]
        });
        // like paths and images: cut to the visible page, and offpage only when none of it is on the page
        // (D94; 003627 p22 draws a glyph across the left edge). An offpage glyph keeps its drawn box.
        let page = [0.0, 0.0, w, h];
        let ink = g.ink.and_then(|k| cut(k, page));
        match cut([g.x0, g.y0, g.x1, g.y1], page) {
            Some((v, cutoff)) => { [g.x0, g.y0, g.x1, g.y1] = v; g.clipped = cutoff; g.ink = ink.map(|(k, _)| k); }
            // the outline can reach the page when the advance box doesn't
            None if ink.is_some() => { g.clipped = true; g.ink = ink.map(|(k, _)| k); }
            None => g.offpage = true,
        }
        if let Some(c) = g.clip {
            let c = page_rect(pb, c);
            match cut([g.x0, g.y0, g.x1, g.y1], c) {
                Some((v, cutoff)) => { [g.x0, g.y0, g.x1, g.y1] = v; g.clipped = cutoff; }
                None => { g.hidden = true; g.clipped = true; }
            }
            // a hidden glyph keeps its drawn box, as above; otherwise the ink box is cut the same way
            if !g.hidden { g.ink = g.ink.and_then(|k| cut(k, c).map(|(v, _)| v)); }
        }
    }
}

/// A word's ink box: its glyphs' boxes with their outlines added (D91), when any outline reaches past
/// the word's own box.
fn word_ink(p: &Page, w: &Word) -> Option<[f64; 4]> {
    let gs = &p.glyphs[w.first..w.first + w.count];
    if gs.iter().all(|g| g.ink.is_none()) { return None; }
    let b = gs.iter().map(|g| g.ink.unwrap_or([g.x0, g.y0, g.x1, g.y1]))
        .fold([w.x0, w.y0, w.x1, w.y1], |a, k| [a[0].min(k[0]), a[1].min(k[1]), a[2].max(k[2]), a[3].max(k[3])]);
    let grown = r1(b[0]) != r1(w.x0) || r1(b[1]) != r1(w.y0) || r1(b[2]) != r1(w.x1) || r1(b[3]) != r1(w.y1);
    if grown { Some(b) } else { None }
}

// regions: placing and merging images

/// A boolean entry, written as `true` (the parser has no boolean value, so read the bytes after the key).
fn flag(dict: &[u8], key: &[u8]) -> bool {
    let mut from = 0;
    while let Some(k) = find(dict, key, from) {
        let e = k + key.len();
        if e < dict.len() && is_regular(dict[e]) { from = e; continue; }
        let v = skip_ws(dict, e);
        return dict[v..].starts_with(b"true");
    }
    false
}

/// The end marker of an inline image's data when its outer filter is ASCII85 (`~>`) or ASCIIHex (`>`).
fn ascii_data_end(dict: &[u8]) -> Option<&'static [u8]> {
    let has = |k: &[u8]| { let mut f = 0; while let Some(p) = find(dict, k, f) { let e = p + k.len(); if e >= dict.len() || !is_regular(dict[e]) { return true; } f = e; } false };
    if has(b"/A85") || has(b"/ASCII85Decode") { Some(b"~>") } else if has(b"/AHx") || has(b"/ASCIIHexDecode") { Some(b">") } else { None }
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 { ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() }

/// Each drawn image's unit square through its matrix and the page box, as a box clipped to the page.
fn place_images(drawn: &[Drawn], pb: &PageBox) -> Vec<Image> {
    let (w, h) = pb.size();
    let mut out = Vec::new();
    for d in drawn {
        let q = [apply(&d.ctm, 0.0, 0.0), apply(&d.ctm, 1.0, 0.0), apply(&d.ctm, 1.0, 1.0), apply(&d.ctm, 0.0, 1.0)];
        let pts: Vec<(f64, f64)> = q.iter().map(|p| pb.map(p.0, p.1)).collect();
        let (bx0, bx1) = (pts.iter().map(|p| p.0).fold(f64::MAX, f64::min), pts.iter().map(|p| p.0).fold(f64::MIN, f64::max));
        let (by0, by1) = (pts.iter().map(|p| p.1).fold(f64::MAX, f64::min), pts.iter().map(|p| p.1).fold(f64::MIN, f64::max));
        let (mut x0, mut y0, mut x1, mut y1) = (bx0.max(0.0), by0.max(0.0), bx1.min(w), by1.min(h));
        // kept in the map, flagged: an image placed off the visible page is still in the file
        let offpage = x1 <= x0 || y1 <= y0;
        if offpage { (x0, y0, x1, y1) = (bx0, by0, bx1, by1); }
        let (mut hidden, mut cut_by_clip) = (false, false);
        if let (false, Some(c)) = (offpage, d.clip) {
            match cut([x0, y0, x1, y1], page_rect(pb, c)) {
                Some((v, c2)) => { [x0, y0, x1, y1] = v; cut_by_clip = c2; }
                None => hidden = true,
            }
        }
        let (side_w, side_h) = (dist(pts[0], pts[1]), dist(pts[0], pts[3]));
        let dpi = |px: u32, len: f64| if len > 1e-9 { px as f64 / (len / 72.0) } else { 0.0 };
        let m = &d.ctm;
        let upright = (m[1].abs() < 1e-6 && m[2].abs() < 1e-6) || (m[0].abs() < 1e-6 && m[3].abs() < 1e-6);
        out.push(Image {
            x0, y0, x1, y1, px_w: d.px_w, px_h: d.px_h, dpi_x: dpi(d.px_w, side_w), dpi_y: dpi(d.px_h, side_h),
            mask: d.mask, inline: d.inline, annot: d.annot, obj: d.obj, parts: 1,
            pieces: vec![Piece { obj: d.obj, bx: [x0, y0, x1, y1], turn: pixels::Turn::from_corners(pts[3], pts[2], pts[0]) }], inline_src: d.src.clone(),
            clipped: offpage || hidden || cut_by_clip || x0 > bx0 + 1e-6 || y0 > by0 + 1e-6 || x1 < bx1 - 1e-6 || y1 < by1 - 1e-6, upright,
            offpage, hidden, order: d.order,
        });
    }
    out
}

/// Join upright images that tile one picture: the same left and right edges and touching top to
/// bottom (horizontal strips), or the same top and bottom and touching side to side. Repeats until
/// nothing joins. Drawing order decides which image keeps its place in the list.
fn merge_strips(mut imgs: Vec<Image>) -> Vec<Image> {
    loop {
        let mut joined = None;
        'outer: for a in 0..imgs.len() {
            for b in 0..imgs.len() {
                if a == b { continue; }
                let (p, q) = (&imgs[a], &imgs[b]);
                if !p.upright || !q.upright || p.offpage || q.offpage || p.hidden || q.hidden || p.mask != q.mask || p.annot != q.annot { continue; }
                let vertical = (p.x0 - q.x0).abs() <= STRIP_EDGE && (p.x1 - q.x1).abs() <= STRIP_EDGE
                    && q.y0 >= p.y0 && (q.y0 - p.y1).abs() <= STRIP_GAP;
                let horizontal = (p.y0 - q.y0).abs() <= STRIP_EDGE && (p.y1 - q.y1).abs() <= STRIP_EDGE
                    && q.x0 >= p.x0 && (q.x0 - p.x1).abs() <= STRIP_GAP;
                if vertical || horizontal { joined = Some((a, b, vertical)); break 'outer; }
            }
        }
        let Some((a, b, vertical)) = joined else { return imgs };
        let q = imgs[b].clone();
        let p = &mut imgs[a];
        if vertical { p.px_h += q.px_h; p.px_w = p.px_w.max(q.px_w); } else { p.px_w += q.px_w; p.px_h = p.px_h.max(q.px_h); }
        p.x0 = p.x0.min(q.x0); p.y0 = p.y0.min(q.y0); p.x1 = p.x1.max(q.x1); p.y1 = p.y1.max(q.y1);
        p.dpi_x = p.dpi_x.min(q.dpi_x); p.dpi_y = p.dpi_y.min(q.dpi_y);
        p.parts += q.parts; p.inline |= q.inline; p.clipped |= q.clipped; p.order = p.order.min(q.order);
        p.pieces.extend(q.pieces);
        p.inline_src = None;
        p.obj = if p.obj == 0 { q.obj } else if q.obj == 0 { p.obj } else { p.obj.min(q.obj) };
        imgs.remove(b);
    }
}

fn is_space(g: &Glyph) -> bool { g.mapped && !g.text.is_empty() && g.text.chars().all(char::is_whitespace) }

/// Glyphs to words, and words to lines, by the fixed rules above. Drawing order is kept.
fn words(glyphs: &[Glyph]) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut cur: Option<(usize, usize)> = None; // (first, last) glyph index of the open word
    let close = |out: &mut Vec<Word>, first: usize, last: usize| {
        let gs = &glyphs[first..=last];
        let text: String = gs.iter().map(|g| if g.mapped { g.text.as_str() } else { "\u{fffd}" }).collect();
        out.push(Word {
            text,
            x0: gs.iter().map(|g| g.x0).fold(f64::MAX, f64::min), y0: gs.iter().map(|g| g.y0).fold(f64::MAX, f64::min),
            x1: gs.iter().map(|g| g.x1).fold(f64::MIN, f64::max), y1: gs.iter().map(|g| g.y1).fold(f64::MIN, f64::max),
            line: 0, font: gs[0].font, size: gs[0].size,
            unmapped: gs.iter().filter(|g| !g.mapped).count(),
            invisible: gs[0].invisible, white: gs[0].white, annot: gs[0].annot, offpage: gs.iter().all(|g| g.offpage), hidden: gs.iter().all(|g| g.hidden),
            first, count: last - first + 1, order: gs.iter().map(|g| g.order).min().unwrap_or(0),
        });
    };
    for (i, g) in glyphs.iter().enumerate() {
        if is_space(g) {
            if let Some((f, l)) = cur.take() { close(&mut out, f, l); }
            continue;
        }
        if let Some((f, l)) = cur {
            let p = &glyphs[l];
            let size = p.size.max(g.size).max(1e-6);
            let (vx, vy) = (g.ux - p.ex, g.uy - p.ey);
            let along = vx * p.dx + vy * p.dy;
            let across = (p.dx * vy - p.dy * vx).abs();
            let same_dir = p.dx * g.dx + p.dy * g.dy > 0.99;
            let same_kind = p.invisible == g.invisible && p.white == g.white && p.annot == g.annot;
            if !same_dir || !same_kind || across > BASELINE_SHIFT * size || along > WORD_GAP * size || along < -BACKSTEP * size {
                close(&mut out, f, l);
                cur = Some((i, i));
            } else {
                cur = Some((f, i));
            }
        } else {
            cur = Some((i, i));
        }
    }
    if let Some((f, l)) = cur { close(&mut out, f, l); }
    // lines: same direction and baseline (within half the font size), and not moving backwards
    let mut line = 0;
    for k in 1..out.len() {
        let (a, b) = (&glyphs[out[k - 1].first + out[k - 1].count - 1], &glyphs[out[k].first]);
        let start = &glyphs[out[k - 1].first];
        let size = a.size.max(b.size).max(1e-6);
        let (vx, vy) = (b.ux - start.ux, b.uy - start.uy);
        let across = (start.dx * vy - start.dy * vx).abs();
        let along = (b.ux - a.ex) * a.dx + (b.uy - a.ey) * a.dy;
        let same_dir = a.dx * b.dx + a.dy * b.dy > 0.99;
        if !same_dir || across > BASELINE_SHIFT * size || along < -BACKSTEP * size { line += 1; }
        out[k].line = line;
    }
    out
}

fn page_content(pdf: &Pdf, page: u32) -> (Vec<u8>, Option<Vec<u8>>) {
    let resources = pdf.inherited(page, b"/Resources").and_then(|v| pdf.resolve(&v));
    let d = match pdf.dict(page) { Some(d) => d, None => return (Vec::new(), resources) };
    // /Contents is a stream, an array of streams, or a reference to an array object holding them
    let refs = match get(&d, b"/Contents") {
        Some(Val::Ref(n)) => match pdf.direct(Val::Ref(n)) { Val::Array(a) => refs_in(&a), _ => vec![n] },
        Some(Val::Array(a)) => refs_in(&a),
        _ => Vec::new(),
    };
    let mut content = Vec::new();
    for r in refs {
        if let Some(b) = pdf.stream(r) { content.extend_from_slice(&b); content.push(b'\n'); }
    }
    (content, resources)
}

/// Extract every glyph and word from a PDF's bytes.
pub fn extract(data: &[u8]) -> Doc {
    if !data.starts_with(b"%PDF") && find(&data[..data.len().min(1024)], b"%PDF", 0).is_none() {
        return Doc { status: "not_pdf", pages: Vec::new(), fonts: Vec::new() };
    }
    let pdf = Pdf::index(data);
    if find(data, b"/Encrypt", 0).is_some() && pdf.crypt.is_none() {
        return Doc { status: "encrypted", pages: Vec::new(), fonts: Vec::new() };
    }
    let mut r = Run { pdf: &pdf, fonts: Fonts { by_obj: HashMap::new(), list: Vec::new() }, out: Vec::new(), annot: false, drawn: Vec::new(), paths: Vec::new(), annots: Vec::new(), seq: 0 };
    let mut pages = Vec::new();
    for (k, p) in pdf.pages().iter().enumerate() {
        let (content, resources) = page_content(&pdf, *p);
        r.seq = 0;
        r.run(&content, resources.as_deref(), GState::default(), 0);
        if let Some(d) = pdf.dict(*p) { r.annotations(&d); }
        let pb = PageBox::of(&pdf, *p);
        let mut glyphs = std::mem::take(&mut r.out);
        place(&mut glyphs, &pb);
        let ws = words(&glyphs);
        let (width, height) = pb.size();
        let v = verdict(&glyphs);
        let images = merge_strips(place_images(&std::mem::take(&mut r.drawn), &pb));
        let regions = ocr::regions(&images, &ws, width, height);
        let lines = map::lines(&ws);
        let paths = place_paths(&std::mem::take(&mut r.paths), &pb);
        let vectors = vector::cluster(&paths, width, height);
        let annots = place_annots(&std::mem::take(&mut r.annots), &pb);
        let map = map::map(&lines, &images, &vectors, &annots);
        pages.push(Page { n: k + 1, width, height, rotate: pb.rotate, glyphs, words: ws, verdict: v, images, regions, lines, paths, vectors, annots, map });
    }
    let fonts = r.fonts.list.iter().map(|f| FontInfo {
        base: f.base.clone(), kind: f.kind, encoding: f.encoding.clone(), to_unicode: f.has_to_unicode(), embedded: f.embedded, widths: f.metrics.source,
    }).collect();
    Doc { status: "ok", pages, fonts }
}

// ---------- JSON ----------

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn r1(v: f64) -> f64 { (v * 10.0).round() / 10.0 }

fn flags(invisible: bool, white: bool, annot: bool, offpage: bool, hidden: bool) -> String {
    let mut s = String::new();
    if invisible { s.push_str(",\"invisible\":true"); }
    if white { s.push_str(",\"white\":true"); }
    if annot { s.push_str(",\"annot\":true"); }
    if offpage { s.push_str(",\"offpage\":true"); }
    if hidden { s.push_str(",\"hidden\":true"); }
    s
}

impl Doc {
    fn fonts_json(&self) -> String {
        let fonts: Vec<String> = self.fonts.iter().map(|f| format!(
            "{{\"base\":{},\"kind\":\"{}\",\"encoding\":{},\"to_unicode\":{},\"embedded\":{},\"widths\":\"{}\"}}",
            json_str(&f.base), f.kind.as_str(), json_str(&f.encoding), f.to_unicode, f.embedded, f.widths
        )).collect();
        fonts.join(",")
    }

    /// The output: per page, its displayed size and every word with its box, in drawing order.
    /// With `glyphs`, each page also lists every glyph with its box and baseline start.
    pub fn to_json(&self, glyphs: bool) -> String {
        let pages: Vec<String> = self.pages.iter().map(|p| {
            let words: Vec<String> = p.words.iter().map(|w| format!(
                "{{\"t\":{},\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{},\"b\":{},\"line\":{},\"font\":{},\"size\":{}{}{}{}}}",
                json_str(&w.text), r1(w.x0), r1(w.y0), r1(w.x1), r1(w.y1), r1(p.glyphs[w.first].oy), w.line,
                if w.font == u32::MAX { -1 } else { w.font as i64 }, r1(w.size),
                match word_ink(p, w) { Some(k) => format!(",\"ink\":[{},{},{},{}]", r1(k[0]), r1(k[1]), r1(k[2]), r1(k[3])), None => String::new() },
                if w.unmapped > 0 { format!(",\"unmapped\":{}", w.unmapped) } else { String::new() },
                flags(w.invisible, w.white, w.annot, w.offpage, w.hidden)
            )).collect();
            let gl = if glyphs {
                let g: Vec<String> = p.glyphs.iter().map(|g| format!(
                    "{{\"c\":{},\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{},\"ox\":{},\"oy\":{},\"size\":{}{}{}}}",
                    json_str(if g.mapped { &g.text } else { "\u{fffd}" }), r1(g.x0), r1(g.y0), r1(g.x1), r1(g.y1), r1(g.ox), r1(g.oy), r1(g.size),
                    if g.mapped { "" } else { ",\"unmapped\":true" }, flags(g.invisible, g.white, g.annot, g.offpage, g.hidden)
                )).collect();
                format!(",\"glyphs\":[{}]", g.join(","))
            } else { String::new() };
            let unmapped = p.glyphs.iter().filter(|g| !g.mapped).count();
            let images: Vec<String> = p.images.iter().map(|m| format!(
                "{{\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{},\"px\":[{},{}],\"dpi\":[{},{}],\"obj\":{},\"parts\":{},\"order\":{}{}{}{}{}{}{}}}",
                r1(m.x0), r1(m.y0), r1(m.x1), r1(m.y1), m.px_w, m.px_h, r1(m.dpi_x), r1(m.dpi_y), m.obj, m.parts, m.order,
                if m.offpage { ",\"offpage\":true" } else if m.hidden { ",\"hidden\":true" } else { "" },
                if m.mask { ",\"mask\":true" } else { "" }, if m.inline { ",\"inline\":true" } else { "" },
                if m.annot { ",\"annot\":true" } else { "" }, if m.clipped { ",\"clipped\":true" } else { "" },
                if m.upright { "" } else { ",\"rotated\":true" }
            )).collect();
            // regions: every image, with its confidence and the reasons that lowered it
            let r3 = |v: f64| (v * 1e3).round() / 1e3;
            let region = |g: &Region| format!(
                "{{\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{},\"confidence\":{},\"reasons\":[{}],\"image\":{},\"obj\":{},\"dpi\":{},\"share\":{},\"text_cover\":{},\"layer_cover\":{},\"text_words\":{},\"layer_words\":{}{}{}{}}}",
                r1(g.x0), r1(g.y0), r1(g.x1), r1(g.y1), r3(g.confidence),
                g.reasons.iter().map(|(w, f)| format!("[\"{}\",{}]", w, r3(*f))).collect::<Vec<_>>().join(","),
                g.image, g.obj, r1(g.dpi), (g.share * 1e4).round() / 1e4,
                r3(g.text_cover), r3(g.layer_cover), g.text_words, g.layer_words,
                if g.mask { ",\"mask\":true" } else { "" }, if g.annot { ",\"annot\":true" } else { "" }, if g.small { ",\"small\":true" } else { "" });
            let regions: Vec<String> = p.regions.iter().map(region).collect();
            let annots: Vec<String> = p.annots.iter().map(|a| format!(
                "{{\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{},\"order\":{},\"subtype\":{},\"field\":{}{}{}{}}}",
                r1(a.x0), r1(a.y0), r1(a.x1), r1(a.y1), a.order, json_str(&a.subtype), json_str(&a.field),
                if a.appearance { ",\"appearance\":true" } else { "" }, if a.hidden { ",\"hidden\":true" } else { "" },
                if a.offpage { ",\"offpage\":true" } else { "" })).collect();
            let paths: Vec<String> = p.paths.iter().map(|q| {
                let mut f = String::new();
                for (on, k) in [(q.fill, "fill"), (q.stroke, "stroke"), (q.shading, "shading"), (q.white, "white"),
                                (q.clipped, "clipped"), (q.offpage, "offpage"), (q.hidden, "hidden"), (q.annot, "annot"),
                                (q.dot, "dot"), (q.empty, "empty")] {
                    if on { f.push_str(&format!(",\"{}\":true", k)); }
                }
                format!("{{\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{},\"order\":{}{}}}", r1(q.x0), r1(q.y0), r1(q.x1), r1(q.y1), q.order, f)
            }).collect();
            // map: every region in drawing order; text by line, with its words as a range into "words"
            let map: Vec<String> = p.map.iter().enumerate().map(|(id, e)| {
                let fl: Vec<String> = e.flags.iter().map(|f| format!("\"{}\"", f)).collect();
                let src = if e.what == "text" {
                    let l = &p.lines[e.index];
                    format!("\"words\":[{},{}],\"t\":{}", l.first, l.count, json_str(&l.text))
                } else if e.what == "annot" {
                    let a = &p.annots[e.index];
                    format!("\"annot\":{},\"subtype\":{},\"field\":{}", e.index, json_str(&a.subtype), json_str(&a.field))
                } else if e.what == "vector" {
                    format!("\"vector\":{},\"paths\":{}", e.index, p.vectors[e.index].paths.len())
                } else {
                    format!("\"image\":{},\"obj\":{}", e.index, p.images[e.index].obj)
                };
                format!("{{\"id\":{},\"what\":\"{}\",\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{},\"order\":{},{},\"flags\":[{}]}}",
                    id, e.what, r1(e.x0), r1(e.y0), r1(e.x1), r1(e.y1), e.order, src, fl.join(","))
            }).collect();
            format!("{{\"n\":{},\"width\":{},\"height\":{},\"rotate\":{},\"verdict\":\"{}\",\"glyph_count\":{},\"unmapped\":{},\"images\":[{}],\"paths\":[{}],\"annots\":[{}],\"map\":[{}],\"regions\":[{}],\"words\":[{}]{}}}",
                p.n, r1(p.width), r1(p.height), p.rotate, p.verdict, p.glyphs.len(), unmapped, images.join(","), paths.join(","), annots.join(","), map.join(","), regions.join(","), words.join(","), gl)
        }).collect();
        format!("{{\"status\":\"{}\",\"pages\":[{}],\"fonts\":[{}]}}", self.status, pages.join(","), self.fonts_json())
    }

    /// Per page: the decoded text in drawing order (unmapped glyphs as U+FFFD) and counts.
    /// Used by tools/decode_check.py.
    pub fn text_json(&self) -> String {
        let pages: Vec<String> = self.pages.iter().map(|p| {
            let text: String = p.glyphs.iter().map(|g| if g.mapped { g.text.as_str() } else { "\u{fffd}" }).collect();
            let unmapped = p.glyphs.iter().filter(|g| !g.mapped).count();
            let invisible = p.glyphs.iter().filter(|g| g.invisible).count();
            let mut by_font: Vec<(u32, usize)> = Vec::new();
            for g in p.glyphs.iter().filter(|g| !g.mapped) {
                match by_font.iter_mut().find(|e| e.0 == g.font) { Some(e) => e.1 += 1, None => by_font.push((g.font, 1)) }
            }
            let uf: Vec<String> = by_font.iter().map(|(f, c)| format!("[{},{}]", if *f == u32::MAX { -1 } else { *f as i64 }, c)).collect();
            format!("{{\"n\":{},\"glyphs\":{},\"unmapped\":{},\"unmapped_by_font\":[{}],\"invisible\":{},\"text\":{}}}",
                p.n, p.glyphs.len(), unmapped, uf.join(","), invisible, json_str(&text))
        }).collect();
        format!("{{\"status\":\"{}\",\"pages\":[{}],\"fonts\":[{}]}}", self.status, pages.join(","), self.fonts_json())
    }
}

// regions: tests of image placement on small hand-written files

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF: `content` drawn on a 600 x 800 page, with image XObjects /A (8 x 4 px) and
    /// /M (a 16 x 16 stencil mask). There's no xref; the index finds objects by scanning.
    fn pdf(content: &str, extra_page: &str) -> Vec<u8> {
        let img = "4 0 obj << /Type /XObject /Subtype /Image /Width 8 /Height 4 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 32 >> stream\n";
        let mask = "5 0 obj << /Type /XObject /Subtype /Image /Width 16 /Height 16 /ImageMask true /Length 32 >> stream\n";
        let mut s = String::from("%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
        s += &format!("2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] {extra_page} /Resources << /XObject << /A 4 0 R /M 5 0 R >> >> /Contents 6 0 R >> endobj\n");
        s += img; s += &"\0".repeat(32); s += "\nendstream endobj\n";
        s += mask; s += &"\0".repeat(32); s += "\nendstream endobj\n";
        // an ICC profile's dictionary, for colour-space tests
        s += "7 0 obj << /N 3 >> endobj\n";
        s += &format!("6 0 obj << /Length {} >> stream\n{}\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n", content.len(), content);
        s.into_bytes()
    }

    fn images(content: &str) -> Vec<Image> { extract(&pdf(content, "")).pages.remove(0).images }

    fn paths(content: &str) -> Vec<Path> { extract(&pdf(content, "")).pages.remove(0).paths }

    /// The page's own /Resources come first in its dictionary, so they're the ones read.
    fn paths_with(content: &str, resources: &str) -> Vec<Path> {
        extract(&pdf(content, &format!("/Resources << {} >>", resources))).pages.remove(0).paths
    }

    fn near(p: &Path, b: [f64; 4]) -> bool {
        (p.x0 - b[0]).abs() < 1e-6 && (p.y0 - b[1]).abs() < 1e-6 && (p.x1 - b[2]).abs() < 1e-6 && (p.y1 - b[3]).abs() < 1e-6
    }

    #[test]
    fn a_glyph_whose_outline_reaches_out_gets_an_ink_box() {
        // 10 pt at (100, 700): advance box x 100..105, ascent 800 and descent -200 give y 92..102 from the
        // top; the outline adds x 99..107 and reaches y 91 (D91). The word's JSON carries both.
        let pdf = font::tiny_truetype_pdf(Some("BT /F1 10 Tf 100 700 Td (A) Tj ET"));
        let doc = extract(&pdf);
        let g = &doc.pages[0].glyphs[0];
        assert!(near_box(g.x0, g.y0, g.x1, g.y1, [100.0, 92.0, 105.0, 102.0]), "{:?}", (g.x0, g.y0, g.x1, g.y1));
        let k = g.ink.unwrap();
        assert!(near_box(k[0], k[1], k[2], k[3], [99.0, 91.0, 107.0, 102.0]), "{k:?}");
        assert!(doc.to_json(false).contains("\"ink\":[99,91,107,102]"));
    }

    #[test]
    fn a_glyph_across_the_page_edge_keeps_its_visible_part() {
        // at x -3 the advance box is -3..2 and its centre is off the page, but its right part shows: cut to
        // 0..2, not offpage; the outline (-4..4) is cut the same way (D94). At x -50 nothing shows.
        let doc = extract(&font::tiny_truetype_pdf(Some("BT /F1 10 Tf -3 700 Td (A) Tj ET")));
        let g = &doc.pages[0].glyphs[0];
        assert!(!g.offpage && near_box(g.x0, g.y0, g.x1, g.y1, [0.0, 92.0, 2.0, 102.0]), "{:?}", (g.offpage, g.x0, g.x1));
        let k = g.ink.unwrap();
        assert!(near_box(k[0], k[1], k[2], k[3], [0.0, 91.0, 4.0, 102.0]), "{k:?}");
        let doc = extract(&font::tiny_truetype_pdf(Some("BT /F1 10 Tf -50 700 Td (A) Tj ET")));
        assert!(doc.pages[0].glyphs[0].offpage && doc.pages[0].words[0].offpage);
    }

    #[test]
    fn a_filled_rectangle_is_a_path() {
        // 100 x 50 at (50, 700) from the bottom: y 50..100 from the top of an 800 pt page
        let v = paths("0 0 1 rg 50 700 100 50 re f");
        assert!(v.len() == 1 && near(&v[0], [50.0, 50.0, 150.0, 100.0]) && v[0].fill && !v[0].stroke && !v[0].white);
        // through the transform
        let v = paths("q 2 0 0 2 0 0 cm 50 350 50 25 re f Q");
        assert!(near(&v[0], [100.0, 50.0, 200.0, 100.0]));
    }

    #[test]
    fn a_stroke_grows_by_half_its_width() {
        let v = paths("4 w 100 400 m 300 400 l S");
        assert!(v.len() == 1 && v[0].stroke && near(&v[0], [100.0, 398.0, 300.0, 402.0]));
        // with square caps the ends reach out too
        let v = paths("4 w 2 J 100 400 m 300 400 l S");
        assert!(near(&v[0], [98.0, 398.0, 302.0, 402.0]));
    }

    #[test]
    fn a_curve_box_is_its_extent_not_its_control_points() {
        // an arch from (100, 400) to (300, 400) with control points at y 500: the top is at y 475
        let v = paths("100 400 m 100 500 300 500 300 400 c f");
        assert!(near(&v[0], [100.0, 325.0, 300.0, 400.0]));
    }

    #[test]
    fn an_unpainted_path_draws_nothing_and_a_clip_limits_what_follows() {
        let v = paths("q 100 100 200 200 re W n 0 0 600 800 re f Q 0 0 10 10 re f");
        assert_eq!(v.len(), 2);
        assert!(near(&v[0], [100.0, 500.0, 300.0, 700.0]) && v[0].clipped);
        // Q restored the clip
        assert!(near(&v[1], [0.0, 790.0, 10.0, 800.0]) && !v[1].clipped);
    }

    #[test]
    fn a_shading_paints_its_clip() {
        let v = paths("q 100 100 200 200 re W n /Sh0 sh Q /Sh0 sh");
        assert!(v.len() == 2 && v[0].shading && near(&v[0], [100.0, 500.0, 300.0, 700.0]));
        assert!(near(&v[1], [0.0, 0.0, 600.0, 800.0]));
    }

    #[test]
    fn white_paint_is_flagged() {
        let v = paths("1 g 50 50 100 100 re f 1 1 1 RG 50 50 100 100 re S 1 g 0 G 50 50 100 100 re B 0 0 0 0 k 50 50 10 10 re f");
        assert!(v[0].white && v[1].white && !v[2].white && v[3].white);
    }

    #[test]
    fn white_in_named_colour_spaces_is_flagged() {
        // 003196, 003420: "/Cs6 cs 1 1 1 scn" with Cs6 an ICCBased /N 3 or a CalRGB space
        let res = "/ColorSpace << /Cs6 [/ICCBased 7 0 R] /Cs1 [/CalRGB << /WhitePoint [0.9505 1 1.089] >>] /Sep [/Separation /Spot /DeviceCMYK 7 0 R] >>";
        let v = paths_with("/Cs6 cs 1 1 1 scn 0 0 10 10 re f /Cs1 cs 1 1 1 sc 0 0 10 10 re f /Cs6 cs 0.9 1 1 scn 0 0 10 10 re f /Sep cs 0 scn 0 0 10 10 re f", res);
        assert!(v[0].white && v[1].white && !v[2].white && !v[3].white);
    }

    #[test]
    fn a_point_is_a_round_dot_or_nothing() {
        // 003828: thousands of "x y m h B*" with round caps; a butt-capped stroke paints nothing, a fill
        // the pixel under the point (ISO 32000-1, 8.5.3.2 and 8.5.3.3)
        let v = paths("4 w 1 J 100 100 m h B* 0 J 100 100 m h S 1 J 100 100 m 100 100 l S 0 J 100 100 m h B*");
        assert!(v[0].dot && !v[0].empty && (v[0].x0 - 98.0).abs() < 1e-6 && (v[0].x1 - 102.0).abs() < 1e-6 && (v[0].y1 - v[0].y0 - 4.0).abs() < 1e-6);
        assert!(!v[1].dot && v[1].empty);
        assert!(v[2].dot && !v[2].empty);
        assert!(!v[3].dot && !v[3].empty);
    }

    #[test]
    fn a_path_off_the_page_is_kept_and_flagged() {
        let v = paths("700 100 50 50 re f");
        assert!(v.len() == 1 && v[0].offpage);
    }

    #[test]
    fn a_clip_cuts_an_image_to_what_shows() {
        // a 100 pt image at the page's bottom-left, clipped to its lower-left 50 pt
        let v = images("q 0 0 50 50 re W n q 100 0 0 100 0 0 cm /A Do Q Q");
        assert!(v.len() == 1 && v[0].clipped && !v[0].hidden);
        assert!((v[0].x0).abs() < 1e-6 && (v[0].x1 - 50.0).abs() < 1e-6 && (v[0].y0 - 750.0).abs() < 1e-6 && (v[0].y1 - 800.0).abs() < 1e-6);
        // a clip elsewhere hides it, and it stays in the map, flagged
        let v = images("q 500 500 10 10 re W n q 100 0 0 100 0 0 cm /A Do Q Q");
        assert!(v.len() == 1 && v[0].hidden && !v[0].offpage);
        let e = extract(&pdf("q 500 500 10 10 re W n q 100 0 0 100 0 0 cm /A Do Q Q", "")).pages.remove(0);
        assert_eq!(e.map[0].flags, ["hidden"]);
    }

    #[test]
    fn a_path_keeps_its_shape_counts() {
        // a rectangle by re; a triangle of lines, closed; an open curve
        let v = paths("0 0 10 10 re f 0 0 m 10 0 l 5 8 l h f 0 0 m 5 5 10 5 15 0 c S");
        assert!(v[0].rect && v[0].closed && v[0].lines == 4 && v[0].curves == 0);
        assert!(!v[1].rect && v[1].closed && v[1].lines == 3);
        assert!(!v[2].closed && v[2].curves == 1 && v[2].lines == 0);
    }

    #[test]
    fn white_text_is_flagged() {
        // 003077: "VerDate" slugs filled white; mode 1 strokes, so the black stroke shows
        let e = extract(&pdf("1 g BT 100 100 Td (a) Tj ET 0 g BT 200 100 Td (b) Tj ET 1 g 0 G 1 Tr BT 300 100 Td (c) Tj ET 1 G 2 Tr BT 400 100 Td (d) Tj ET", "")).pages.remove(0);
        let w: Vec<bool> = e.words.iter().map(|w| w.white).collect();
        assert_eq!(w, [true, false, false, true]);
        // no font, so the glyphs are also undecodable
        assert!(e.map[0].flags.contains(&"white") && !e.map[1].flags.contains(&"white"));
    }

    #[test]
    fn a_clip_hides_text_outside_it() {
        // no font, so the glyph is a point at its origin; the clip is far from it
        let e = extract(&pdf("q 0 0 10 10 re W n BT 100 100 Td (a) Tj ET Q BT 5 5 Td (b) Tj ET", "")).pages.remove(0);
        assert!(e.glyphs[0].hidden && !e.glyphs[1].hidden && e.words[0].hidden && !e.words[1].hidden);
    }

    fn annot_pdf() -> Vec<u8> {
        // a link with no appearance, and a hidden text field whose /FT is on its parent
        let s = "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
            2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
            3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Annots [4 0 R 5 0 R] /Contents 7 0 R >> endobj\n\
            4 0 obj << /Type /Annot /Subtype /Link /Rect [100 700 200 720] >> endobj\n\
            5 0 obj << /Type /Annot /Subtype /Widget /F 2 /Parent 6 0 R /Rect [150 70 50 50] >> endobj\n\
            6 0 obj << /FT /Tx /T (name) /Kids [5 0 R] >> endobj\n\
            7 0 obj << /Length 0 >> stream\n\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n";
        s.as_bytes().to_vec()
    }

    #[test]
    fn every_annotation_is_a_region() {
        let p = extract(&annot_pdf()).pages.remove(0);
        assert_eq!(p.annots.len(), 2);
        let (l, w) = (&p.annots[0], &p.annots[1]);
        assert!(l.subtype == "Link" && !l.appearance && !l.hidden && near_box(l.x0, l.y0, l.x1, l.y1, [100.0, 80.0, 200.0, 100.0]));
        assert!(w.subtype == "Widget" && w.field == "Tx" && w.hidden && near_box(w.x0, w.y0, w.x1, w.y1, [50.0, 730.0, 150.0, 750.0]));
        let kinds: Vec<&str> = p.map.iter().map(|m| m.what).collect();
        assert_eq!(kinds, ["annot", "annot"]);
        assert!(p.map[1].flags.contains(&"hidden") && p.map[0].flags.contains(&"no_appearance"));
    }

    fn near_box(x0: f64, y0: f64, x1: f64, y1: f64, b: [f64; 4]) -> bool {
        (x0 - b[0]).abs() < 1e-6 && (y0 - b[1]).abs() < 1e-6 && (x1 - b[2]).abs() < 1e-6 && (y1 - b[3]).abs() < 1e-6
    }

    #[test]
    fn paths_share_the_drawing_order() {
        let e = extract(&pdf("0 0 600 800 re f q 10 0 0 10 100 100 cm /A Do Q 50 50 10 10 re f", "")).pages.remove(0);
        assert!(e.paths[0].order == 0 && e.images[0].order == 1 && e.paths[1].order == 2);
        let kinds: Vec<&str> = e.map.iter().map(|m| m.what).collect();
        assert_eq!(kinds, ["vector", "image", "vector"]);
    }

    #[test]
    fn xobject_box_and_dpi() {
        // 72 x 36 points at (100, 700) from the bottom: top-left origin puts it at y 64..100
        let v = images("q 72 0 0 36 100 700 cm /A Do Q");
        assert_eq!(v.len(), 1);
        let m = &v[0];
        assert!((m.x0 - 100.0).abs() < 1e-6 && (m.x1 - 172.0).abs() < 1e-6 && (m.y0 - 64.0).abs() < 1e-6 && (m.y1 - 100.0).abs() < 1e-6);
        assert!((m.dpi_x - 8.0).abs() < 1e-6 && (m.dpi_y - 8.0).abs() < 1e-6);
        assert!(m.upright && !m.mask && !m.inline && !m.clipped && m.obj == 4);
    }

    #[test]
    fn q_restores_the_matrix() {
        let v = images("q 2 0 0 2 0 0 cm q 10 0 0 10 0 0 cm Q Q q 10 0 0 10 50 50 cm /A Do Q");
        assert!((v[0].x0 - 50.0).abs() < 1e-6 && (v[0].x1 - 60.0).abs() < 1e-6);
    }

    #[test]
    fn rotated_image_is_boxed_and_flagged() {
        // 90 degrees: the unit square turned, 40 wide by 20 high on the page
        let v = images("q 0 20 -40 0 300 400 cm /A Do Q");
        let m = &v[0];
        assert!(m.upright); // a quarter turn is still axis-aligned
        assert!((m.x1 - m.x0 - 40.0).abs() < 1e-6 && (m.y1 - m.y0 - 20.0).abs() < 1e-6);
        // the image's own width (8 px) runs along the 20 pt side
        assert!((m.dpi_x - 8.0 / (20.0 / 72.0)).abs() < 1e-6);
        let skew = images("q 40 0 10 20 300 400 cm /A Do Q");
        assert!(!skew[0].upright);
    }

    #[test]
    fn clipped_to_the_page() {
        let v = images("q 100 0 0 100 550 -50 cm /A Do Q");
        let m = &v[0];
        assert!(m.clipped && (m.x1 - 600.0).abs() < 1e-6 && (m.y1 - 800.0).abs() < 1e-6);
        // wholly off the page: kept and flagged, with its drawn box
        let off = images("q 10 0 0 10 700 100 cm /A Do Q");
        assert!(off.len() == 1 && off[0].offpage && off[0].clipped && (off[0].x0 - 700.0).abs() < 1e-6);
        assert!(!m.offpage);
    }

    #[test]
    fn drawing_order_is_counted_across_glyphs_and_images() {
        let v = images("q 10 0 0 10 100 100 cm /A Do Q q 10 0 0 10 300 100 cm /A Do Q");
        assert!(v.len() == 2 && v[0].order == 0 && v[1].order == 1);
    }

    #[test]
    fn crop_box_moves_the_origin() {
        let d = extract(&pdf("q 10 0 0 10 110 110 cm /A Do Q", "/CropBox [100 100 500 700]"));
        let m = &d.pages[0].images[0];
        assert!((m.x0 - 10.0).abs() < 1e-6 && (m.y1 - 590.0).abs() < 1e-6);
    }

    #[test]
    fn stencil_mask_is_flagged() {
        assert!(images("q 16 0 0 16 0 0 cm /M Do Q")[0].mask);
    }

    #[test]
    fn inline_image_with_short_keys() {
        let v = images("q 30 0 0 15 10 10 cm BI /W 6 /H 3 /CS /G /BPC 8 ID \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0 EI Q q 5 0 0 5 0 0 cm BI /IM true /W 8 /H 8 ID \0\0\0\0\0\0\0\0 EI Q");
        assert_eq!(v.len(), 2);
        assert!(v[0].inline && v[0].px_w == 6 && v[0].px_h == 3 && v[0].obj == 0 && !v[0].mask);
        assert!((v[0].dpi_x - 6.0 / (30.0 / 72.0)).abs() < 1e-6);
        assert!(v[1].mask);
        // the dictionary and data are kept for the pixels: 18 zero bytes, without the white space around them
        let src = v[0].inline_src.as_ref().unwrap();
        assert!(src.0.windows(5).any(|w| w == b"/BPC ") && src.1 == vec![0u8; 18]);
    }

    #[test]
    fn ascii85_inline_data_holding_ei() {
        // the A85 data has a line starting "EI(": without skipping to ~> the second image is lost
        let v = images("q 10 0 0 10 10 10 cm BI /W 2 /H 1 /CS /G /BPC 8 /F /A85 ID ab
EI(cd~> EI Q q 10 0 0 10 50 50 cm BI /W 2 /H 1 /CS /G /BPC 8 /F [/A85 /Fl] ID xy~> EI Q");
        assert_eq!(v.len(), 2);
        assert!((v[1].x0 - 50.0).abs() < 1e-6);
    }

    #[test]
    fn image_inside_a_form() {
        let mut f = String::from_utf8(pdf("q 2 0 0 2 10 10 cm /F Do Q", "")).unwrap();
        f = f.replace("/A 4 0 R /M 5 0 R", "/A 4 0 R /M 5 0 R /F 7 0 R");
        let body = "q 20 0 0 10 5 5 cm /A Do Q";
        f = f.replace("trailer", &format!("7 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Matrix [1 0 0 1 100 0] /Resources << /XObject << /A 4 0 R >> >> /Length {} >> stream\n{}\nendstream endobj\ntrailer", body.len(), body));
        let v = extract(f.as_bytes()).pages.remove(0).images;
        // form matrix, then the page cm: x = 2 * (100 + 5) + 10
        assert_eq!(v.len(), 1);
        assert!((v[0].x0 - 220.0).abs() < 1e-6 && (v[0].x1 - 260.0).abs() < 1e-6);
    }

    #[test]
    fn strips_merge_in_both_directions() {
        let v = images("q 60 0 0 10 100 500 cm /A Do Q q 60 0 0 10 100 490 cm /A Do Q q 60 0 0 10 100 480 cm /A Do Q");
        assert_eq!(v.len(), 1);
        assert!(v[0].parts == 3 && v[0].px_h == 12 && (v[0].y1 - v[0].y0 - 30.0).abs() < 1e-6);
        let h = images("q 10 0 0 40 100 100 cm /A Do Q q 10 0 0 40 110 100 cm /A Do Q");
        assert!(h.len() == 1 && h[0].parts == 2 && h[0].px_w == 16);
    }

    #[test]
    fn apart_or_misaligned_images_stay_apart() {
        // a 5 pt gap, and a strip one point narrower than the other
        assert_eq!(images("q 60 0 0 10 100 500 cm /A Do Q q 60 0 0 10 100 485 cm /A Do Q").len(), 2);
        assert_eq!(images("q 60 0 0 10 100 500 cm /A Do Q q 59 0 0 10 100 490 cm /A Do Q").len(), 2);
        // a mask and an image side by side don't join
        assert_eq!(images("q 10 0 0 10 0 0 cm /A Do Q q 10 0 0 10 10 0 cm /M Do Q").len(), 2);
    }
}
