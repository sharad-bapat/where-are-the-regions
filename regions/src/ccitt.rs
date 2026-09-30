//! CCITT fax decoding for CCITTFaxDecode images (ISO 32000-1 7.4.6; ITU-T T.4 and T.6): Group 4
//! (K < 0), Group 3 one-dimensional (K = 0) and mixed (K > 0). The output is 1 bit a pixel, rows
//! padded to a byte, 0 black unless /BlackIs1, as the filter's output is defined. A row that can't
//! be decoded ends the image; that row and the rest stay white, and the result says so.
//! The run-length codes come from tables.rs (generated from pdfminer's copy of the T.4 tables).

use crate::tables::{CCITT_BLACK, CCITT_WHITE};
use std::sync::OnceLock;

pub struct Params {
    pub k: i32,
    pub columns: usize,
    pub rows: usize,
    pub byte_align: bool,
    pub black_is_1: bool,
}

/// The decoded rows, and whether every row decoded.
pub fn decode(data: &[u8], p: &Params) -> (Vec<u8>, bool) {
    let cols = p.columns;
    let stride = (cols + 7) / 8;
    let white = if p.black_is_1 { 0x00 } else { 0xFF };
    let mut out = vec![white; stride * p.rows];
    let mut bits = Bits { d: data, pos: 0 };
    // the reference line's changing elements, then two sentinels at the right edge
    let mut refl = vec![cols, cols];
    let mut complete = true;
    for row in 0..p.rows {
        let two_d = if p.k < 0 {
            if p.byte_align { bits.align(); }
            true
        } else {
            // Group 3: an optional EOL (with fill zeros before it), then for K > 0 a tag bit
            bits.skip_eol(p.byte_align);
            if p.k > 0 { bits.bit() == Some(0) } else { false }
        };
        let cur = if two_d { row_2d(&mut bits, &refl, cols) } else { row_1d(&mut bits, cols) };
        let Some(cur) = cur else { complete = false; break };
        paint(&mut out[row * stride..(row + 1) * stride], &cur, cols, p.black_is_1);
        refl = cur;
        refl.push(cols);
        refl.push(cols);
    }
    (out, complete)
}

/// Black pixels are between each pair of changing elements (the first change is to black).
fn paint(row: &mut [u8], changes: &[usize], cols: usize, black_is_1: bool) {
    for pair in changes.chunks(2) {
        let (a, b) = (pair[0].min(cols), pair.get(1).copied().unwrap_or(cols).min(cols));
        for x in a..b {
            let mask = 0x80u8 >> (x % 8);
            if black_is_1 { row[x / 8] |= mask; } else { row[x / 8] &= !mask; }
        }
    }
}

/// One two-dimensionally coded row (T.4 4.2, T.6): pass, horizontal and vertical modes against the
/// reference line. Returns the row's changing elements.
fn row_2d(bits: &mut Bits, refl: &[usize], cols: usize) -> Option<Vec<usize>> {
    let mut cur = Vec::new();
    let (mut a0, mut black) = (-1isize, false);
    while a0 < cols as isize {
        // b1: the first changing element on the reference line right of a0 and of the other colour
        // than a0's (elements at even indexes change to black); b2 the next one
        let mut i = 0;
        while i < refl.len() && (refl[i] as isize <= a0 || (i % 2 == 1) != black) { i += 1; }
        let b1 = refl.get(i).copied().unwrap_or(cols) as isize;
        let b2 = refl.get(i + 1).copied().unwrap_or(cols) as isize;
        match mode(bits)? {
            Mode::Pass => a0 = b2,
            Mode::Horizontal => {
                let start = a0.max(0);
                let r1 = run(bits, !black)? as isize;
                let r2 = run(bits, black)? as isize;
                let (a1, a2) = (start + r1, start + r1 + r2);
                cur.push(a1.min(cols as isize) as usize);
                cur.push(a2.min(cols as isize) as usize);
                a0 = a2;
            }
            Mode::Vertical(d) => {
                let a1 = b1 + d as isize;
                if a1 < 0 || a1 < a0 || a1 > cols as isize { return None; }
                cur.push(a1 as usize);
                a0 = a1;
                black = !black;
            }
        }
    }
    Some(cur)
}

/// One one-dimensionally coded row (T.4 4.1): runs alternating white and black.
fn row_1d(bits: &mut Bits, cols: usize) -> Option<Vec<usize>> {
    let (mut cur, mut x, mut white) = (Vec::new(), 0usize, true);
    while x < cols {
        x += run(bits, white)?;
        if x < cols { cur.push(x); }
        white = !white;
    }
    Some(cur)
}

enum Mode { Pass, Horizontal, Vertical(i8) }

fn mode(bits: &mut Bits) -> Option<Mode> {
    let p = bits.peek(7);
    let (len, m) = if p >> 6 == 1 { (1, Mode::Vertical(0)) }
        else if p >> 4 == 0b011 { (3, Mode::Vertical(1)) }
        else if p >> 4 == 0b010 { (3, Mode::Vertical(-1)) }
        else if p >> 4 == 0b001 { (3, Mode::Horizontal) }
        else if p >> 3 == 0b0001 { (4, Mode::Pass) }
        else if p >> 1 == 0b000011 { (6, Mode::Vertical(2)) }
        else if p >> 1 == 0b000010 { (6, Mode::Vertical(-2)) }
        else if p == 0b0000011 { (7, Mode::Vertical(3)) }
        else if p == 0b0000010 { (7, Mode::Vertical(-3)) }
        else { return None }; // an EOL, an extension or damage
    bits.skip(len);
    Some(m)
}

/// A run length: makeup codes (64 and up) until a terminating code (under 64).
fn run(bits: &mut Bits, white: bool) -> Option<usize> {
    let t = if white { lut(&WHITE, &CCITT_WHITE) } else { lut(&BLACK, &CCITT_BLACK) };
    let mut total = 0usize;
    loop {
        let (len, r) = t[bits.peek(13) as usize];
        if len == 0 || bits.left() < len as usize { return None; }
        bits.skip(len as usize);
        total += r as usize;
        if r < 64 { return Some(total); }
    }
}

static WHITE: OnceLock<Vec<(u8, u16)>> = OnceLock::new();
static BLACK: OnceLock<Vec<(u8, u16)>> = OnceLock::new();

/// A table indexed by the next 13 bits: (code length, run), length 0 for no code.
fn lut<'a>(cell: &'a OnceLock<Vec<(u8, u16)>>, codes: &[(u16, u8, u16)]) -> &'a [(u8, u16)] {
    cell.get_or_init(|| {
        let mut t = vec![(0u8, 0u16); 1 << 13];
        for &(code, len, run) in codes {
            let shift = 13 - len as u32;
            let base = (code as usize) << shift;
            for e in t.iter_mut().skip(base).take(1 << shift) { *e = (len, run); }
        }
        t
    })
}

struct Bits<'a> { d: &'a [u8], pos: usize }

impl Bits<'_> {
    fn left(&self) -> usize { (self.d.len() * 8).saturating_sub(self.pos) }
    /// The next n bits (up to 16), zeros past the end.
    fn peek(&self, n: usize) -> u32 {
        let mut v = 0u32;
        for k in 0..n {
            let p = self.pos + k;
            let b = self.d.get(p / 8).map_or(0, |&x| (x >> (7 - p % 8)) & 1);
            v = (v << 1) | b as u32;
        }
        v
    }
    fn skip(&mut self, n: usize) { self.pos += n; }
    fn bit(&mut self) -> Option<u32> {
        if self.left() == 0 { return None; }
        let b = self.peek(1);
        self.pos += 1;
        Some(b)
    }
    fn align(&mut self) { self.pos = (self.pos + 7) / 8 * 8; }
    /// Skip an EOL (eleven or more zeros, then a one) if one comes next.
    fn skip_eol(&mut self, _byte_align: bool) {
        let mut z = 0;
        while z < 64 && self.left() > z && self.peek_at(z) == 0 { z += 1; }
        if z >= 11 && self.left() > z { self.pos += z + 1; }
    }
    fn peek_at(&self, k: usize) -> u32 {
        let p = self.pos + k;
        self.d.get(p / 8).map_or(0, |&x| ((x >> (7 - p % 8)) & 1) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g4(data: &[u8], cols: usize, rows: usize) -> (Vec<u8>, bool) {
        decode(data, &Params { k: -1, columns: cols, rows, byte_align: false, black_is_1: false })
    }

    #[test]
    fn white_rows_are_one_bit_each() {
        // against an all-white reference line, V0 (a single 1 bit) codes an all-white row
        let (out, ok) = g4(&[0xFF], 8, 8);
        assert!(ok && out == [0xFF; 8]);
    }

    #[test]
    fn horizontal_mode_codes_two_runs() {
        // H, white 2 (0111), black 4 (011), then V0 to the edge: pixels 2 to 5 black
        let (out, ok) = g4(&[0b0010_1110, 0b1110_0000], 8, 1);
        assert!(ok);
        assert_eq!(out, [0b1100_0011]);
        // the next row repeats it with V0, V0 (the changes line up with the reference line)
        let (out, ok) = g4(&[0b0010_1110, 0b1111_1100], 8, 2);
        assert!(ok);
        assert_eq!(out, [0b1100_0011, 0b1100_0011]);
    }

    #[test]
    fn damage_ends_the_image_white() {
        // an EOL where a mode should be: nothing decodes, every row stays white, and it says so
        let (out, ok) = g4(&[0x00, 0x10], 8, 2);
        assert!(!ok && out == [0xFF, 0xFF]);
    }

    #[test]
    fn group_3_one_dimensional_rows() {
        // K = 0: EOL, then white 2, black 4, white 2 (0111, 011, 0111)
        let data = [0b0000_0000, 0b0001_0111, 0b0110_1110];
        let (out, ok) = decode(&data, &Params { k: 0, columns: 8, rows: 1, byte_align: false, black_is_1: false });
        assert!(ok);
        assert_eq!(out, [0b1100_0011]);
    }
}
