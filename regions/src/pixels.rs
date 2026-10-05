//! Pixels of an image XObject, as a small grey thumbnail, for the kind layer (plans/chunk8-kind-images.md).
//!
//! The stream is decompressed by `Pdf::stream` (Flate, LZW, RunLength, ASCII85, ASCIIHex), then this
//! module undoes a PNG or TIFF predictor, reads the samples by the colour space, bit depth and /Decode
//! array, turns each pixel grey (0 black to 255 white) and averages square blocks down to at most
//! THUMB pixels on the long side. Integer arithmetic throughout, so native and wasm agree.
//! DCTDecode (JPEG) goes through the zune-jpeg crate (D84), without SIMD so native and wasm agree.
//! CCITTFaxDecode goes through ccitt.rs (our own Group 3 and 4 decoder). JPX and JBIG2 stay
//! undecoded and give an error naming them (D84).

use crate::{get, parse_val, skip_val, skip_ws, Pdf, Val};

/// Longest side of a thumbnail, in pixels.
pub const THUMB: u32 = 256;

pub struct Thumb {
    /// The image's own size.
    pub width: u32, pub height: u32,
    /// The thumbnail's size and its grey levels, row by row, 0 black to 255 white.
    pub w: u32, pub h: u32,
    pub grey: Vec<u8>,
}

/// Which way an image's stored pixels face on the displayed page, the page's /Rotate and the image's
/// matrix together, to the nearest quarter turn: rows and columns swapped, then each displayed axis
/// reversed. The kind layer reads rows of text left to right, so a scan stored on its side and
/// turned by /Rotate has to be turned the same way first (results/kinds-turn.md).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turn { pub transpose: bool, pub flip_x: bool, pub flip_y: bool }

impl Turn {
    /// From where the image's unit square lands on the displayed page (top-left origin): its first
    /// stored pixel's corner (image space 0,1), the end of that first row (1,1) and the start of the
    /// last row (0,0).
    pub fn from_corners(first: (f64, f64), row_end: (f64, f64), last_row: (f64, f64)) -> Turn {
        let (ux, uy) = (row_end.0 - first.0, row_end.1 - first.1);
        let (vx, vy) = (last_row.0 - first.0, last_row.1 - first.1);
        if ux.abs() >= uy.abs() { Turn { transpose: false, flip_x: ux < 0.0, flip_y: vy < 0.0 } }
        else { Turn { transpose: true, flip_x: vx < 0.0, flip_y: uy < 0.0 } }
    }

    /// The thumbnail as it shows on the page. An upright image comes back untouched.
    pub fn apply(self, t: Thumb) -> Thumb {
        if self == Turn::default() { return t; }
        let (tw, th) = (t.w as usize, t.h as usize);
        let (dw, dh) = if self.transpose { (th, tw) } else { (tw, th) };
        let mut grey = vec![255u8; dw * dh];
        for y in 0..dh {
            for x in 0..dw {
                let (x2, y2) = (if self.flip_x { dw - 1 - x } else { x }, if self.flip_y { dh - 1 - y } else { y });
                let (sx, sy) = if self.transpose { (y2, x2) } else { (x2, y2) };
                grey[y * dw + x] = t.grey[sy * tw + sx];
            }
        }
        let (width, height) = if self.transpose { (t.height, t.width) } else { (t.width, t.height) };
        Thumb { width, height, w: dw as u32, h: dh as u32, grey }
    }
}

/// The thumbnail of a placed image as it shows on the page, at most `max` px on the long side: an
/// inline image from its source, one object, or merged strips pasted together, each turned by its
/// Turn. The one entry point for the kind layer and the router.
pub fn placed_thumbnail(data: &[u8], img: &crate::Image, max: u32) -> Result<Thumb, &'static str> {
    if let Some(src) = &img.inline_src {
        let t = inline_thumbnail(&src.0, &src.1, max)?;
        return Ok(match img.pieces.first() { Some(p) => p.turn.apply(t), None => t });
    }
    let objs: Vec<&crate::Piece> = img.pieces.iter().filter(|p| p.obj != 0).collect();
    match objs.len() {
        0 => Err("no_object"),
        1 if img.pieces.len() == 1 => image_thumbnail_max(data, objs[0].obj, max).map(|t| objs[0].turn.apply(t)),
        _ => merged_thumbnail(data, &img.pieces, max),
    }
}

/// The thumbnail of image XObject `obj` in the PDF `data`, at most THUMB pixels on the long side.
pub fn image_thumbnail(data: &[u8], obj: u32) -> Result<Thumb, &'static str> { image_thumbnail_max(data, obj, THUMB) }

/// The same, at most `max` pixels on the long side (the kind layer uses kind::KIND_THUMB).
pub fn image_thumbnail_max(data: &[u8], obj: u32, max: u32) -> Result<Thumb, &'static str> {
    let pdf = Pdf::index(data);
    thumbnail(&pdf, obj, max.max(1))
}

/// One thumbnail for an image drawn as several pieces (merged strips): each piece's thumbnail is
/// pasted where the piece sits in the merged box, at the pieces' own resolution (the coarsest), capped
/// at `max` px on the long side. Inline pieces, and pieces that don't decode, stay white; it fails
/// only when no piece decodes.
pub fn merged_thumbnail(data: &[u8], pieces: &[crate::Piece], max: u32) -> Result<Thumb, &'static str> {
    let pdf = Pdf::index(data);
    let bx = pieces.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, q| { let p = q.bx; [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[2]), b[3].max(p[3])] });
    let (bw, bh) = (bx[2] - bx[0], bx[3] - bx[1]);
    if bw <= 0.0 || bh <= 0.0 { return Err("no_size"); }
    // pixels per point: the coarsest piece's own, and no more than the cap allows
    let mut ppp = f64::MAX;
    let mut first_err = "undecoded";
    let mut thumbs = Vec::new();
    for q in pieces {
        let (obj, b) = (q.obj, q.bx);
        if obj == 0 { continue; }
        // turned first, so each piece's width and height are the ones its box on the page has
        match thumbnail(&pdf, obj, max.max(1)).map(|t| q.turn.apply(t)) {
            Ok(t) => {
                let (pw, ph) = (b[2] - b[0], b[3] - b[1]);
                if pw > 0.0 && ph > 0.0 { ppp = ppp.min(t.width as f64 / pw).min(t.height as f64 / ph); }
                thumbs.push((t, b));
            }
            Err(e) => first_err = e,
        }
    }
    if thumbs.is_empty() { return Err(first_err); }
    ppp = ppp.min(max as f64 / bw.max(bh)).max(1e-6);
    let (w, h) = (((bw * ppp).round() as u32).max(1), ((bh * ppp).round() as u32).max(1));
    let mut grey = vec![255u8; (w * h) as usize];
    for (t, b) in &thumbs {
        // the piece's place on the canvas, filled from its thumbnail by nearest pixel
        let (x0, y0) = (((b[0] - bx[0]) * ppp).round() as i64, ((b[1] - bx[1]) * ppp).round() as i64);
        let (x1, y1) = (((b[2] - bx[0]) * ppp).round() as i64, ((b[3] - bx[1]) * ppp).round() as i64);
        let (rw, rh) = ((x1 - x0).max(1), (y1 - y0).max(1));
        for y in y0.max(0)..y1.min(h as i64) {
            let sy = (((y - y0) * t.h as i64) / rh).min(t.h as i64 - 1) as usize;
            for x in x0.max(0)..x1.min(w as i64) {
                let sx = (((x - x0) * t.w as i64) / rw).min(t.w as i64 - 1) as usize;
                grey[y as usize * w as usize + x as usize] = t.grey[sy * t.w as usize + sx];
            }
        }
    }
    let (width, height) = thumbs.iter().fold((0u32, 0u32), |(a, c), (t, _)| (a.max(t.width), c + t.height));
    Ok(Thumb { width, height, w, h, grey })
}

/// The thumbnail of an inline image from its dictionary (the bytes between BI and ID, short keys
/// allowed) and its data (between ID and EI). A colour space named from the page's resources isn't
/// looked up, and gives an error.
pub fn inline_thumbnail(dict: &[u8], data: &[u8], max: u32) -> Result<Thumb, &'static str> {
    let d = full_keys(dict);
    let pdf = Pdf::index(b"%PDF-1.4\n%%EOF\n");
    let (raw, codec) = pdf.unfilter(&d, data.to_vec(), true).ok_or("undecoded")?;
    from_parts(&pdf, &d, raw, codec, max.max(1))
}

/// An inline image's dictionary with its short keys spelt out (ISO 32000-1 8.9.7, table 93), as an
/// ordinary "<< ... >>" dictionary. Short filter and colour space names are read as they are.
fn full_keys(dict: &[u8]) -> Vec<u8> {
    let mut out = b"<<".to_vec();
    let mut i = skip_ws(dict, 0);
    while i < dict.len() {
        let Val::Name(k) = parse_val(dict, i) else { break };
        let v = skip_ws(dict, skip_val(dict, i));
        let e = skip_val(dict, v).max(v + 1).min(dict.len());
        let full: &[u8] = match k.as_slice() {
            b"W" => b"Width", b"H" => b"Height", b"BPC" => b"BitsPerComponent", b"CS" => b"ColorSpace", b"D" => b"Decode",
            b"DP" => b"DecodeParms", b"F" => b"Filter", b"IM" => b"ImageMask", b"I" => b"Interpolate", other => other,
        };
        out.extend(b" /");
        out.extend(full);
        out.push(b' ');
        out.extend(&dict[v..e]);
        i = skip_ws(dict, e);
    }
    out.extend(b" >>");
    out
}

pub(crate) fn thumbnail(pdf: &Pdf, obj: u32, limit: u32) -> Result<Thumb, &'static str> {
    let d = pdf.dict(obj).ok_or("no_object")?;
    let int = |k: &[u8]| match get(&d, k).map(|v| pdf.direct(v)) { Some(Val::Num(x)) if x > 0.0 => x as u32, _ => 0 };
    if int(b"/Width") == 0 || int(b"/Height") == 0 { return Err("no_size"); }
    let (raw, codec) = pdf.decode(obj, true).ok_or("undecoded")?;
    from_parts(pdf, &d, raw, codec, limit)
}

/// The thumbnail from an image's dictionary and its data run through every filter but a last image codec.
fn from_parts(pdf: &Pdf, d: &[u8], raw: Vec<u8>, codec: Option<Vec<u8>>, limit: u32) -> Result<Thumb, &'static str> {
    let d = d.to_vec();
    let int = |k: &[u8]| match get(&d, k).map(|v| pdf.direct(v)) { Some(Val::Num(x)) if x > 0.0 => x as u32, _ => 0 };
    let (width, height) = (int(b"/Width"), int(b"/Height"));
    if width == 0 || height == 0 { return Err("no_size"); }
    let dec: Vec<f64> = match get(&d, b"/Decode").map(|v| pdf.direct(v)) {
        Some(Val::Array(a)) => nums(&a),
        _ => Vec::new(),
    };
    let mask = crate::find(&d, b"/ImageMask true", 0).is_some();
    let (data, space, bpc, width, height) = match codec.as_deref() {
        Some(b"DCTDecode") | Some(b"DCT") => {
            let (data, space, w, h) = jpeg(&raw)?;
            (data, space, 8, w, h)
        }
        Some(b"CCITTFaxDecode") | Some(b"CCF") => {
            // the fax output is 1 bit a pixel, read by the image's colour space (or as a mask) as usual
            let parms = last_parms(pdf, get(&d, b"/DecodeParms"));
            let p = parms.as_deref().unwrap_or(b"<< >>");
            let num = |k: &[u8], dflt: f64| match get(p, k).map(|v| pdf.direct(v)) { Some(Val::Num(x)) => x, _ => dflt };
            let flag = |k: &[u8]| crate::find(p, &[k, b" true"].concat(), 0).is_some();
            let columns = num(b"/Columns", 1728.0) as usize;
            if columns != width as usize { return Err("ccitt_columns"); }
            let rows = match num(b"/Rows", 0.0) as usize { 0 => height as usize, r => r.min(height as usize) };
            let params = crate::ccitt::Params { k: num(b"/K", 0.0) as i32, columns, rows, byte_align: flag(b"/EncodedByteAlign"), black_is_1: flag(b"/BlackIs1") };
            let (mut data, _complete) = crate::ccitt::decode(&raw, &params);
            // rows the file doesn't code (fewer /Rows than /Height) are white
            let white = if params.black_is_1 { 0x00 } else { 0xFF };
            data.resize((columns + 7) / 8 * height as usize, white);
            let space = if mask { Space::Mask } else { space_of(pdf, get(&d, b"/ColorSpace")).unwrap_or(Space::Gray) };
            if space.components() != 1 { return Err("colour_space"); }
            (data, space, 1, width, height)
        }
        Some(b"JPXDecode") => return Err("jpx"),
        Some(b"JBIG2Decode") => return Err("jbig2"),
        Some(_) => return Err("undecoded"),
        None => {
            let bpc = if mask { 1 } else { match int(b"/BitsPerComponent") { 0 => 8, b => b } };
            if ![1, 2, 4, 8, 16].contains(&bpc) { return Err("bits"); }
            let space = if mask { Space::Mask } else { space_of(pdf, get(&d, b"/ColorSpace"))? };
            let row = ((width as usize) * space.components() * bpc as usize + 7) / 8;
            let parms = last_parms(pdf, get(&d, b"/DecodeParms"));
            let data = unpredict(pdf, parms.as_deref(), raw, row, space.components(), bpc as usize)?;
            if data.len() < row * height as usize { return Err("short"); }
            (data, space, bpc, width, height)
        }
    };
    let n = space.components();
    let row = ((width as usize) * n * bpc as usize + 7) / 8;
    if data.len() < row * height as usize { return Err("short"); }

    // /Decode: per component a (min, max) pair; an inverted pair flips the component.
    // 16-bit samples are read by their top byte, so they count as 8-bit here.
    let max = if bpc == 16 { 255 } else { (1u32 << bpc) - 1 };
    let flip: Vec<bool> = (0..n).map(|c| dec.len() >= 2 * (c + 1) && dec[2 * c] > dec[2 * c + 1]).collect();

    let f = ((width.max(height) + limit - 1) / limit).max(1);
    let (w, h) = ((width + f - 1) / f, (height + f - 1) / f);
    let mut sum = vec![0u32; (w * h) as usize];
    let mut cnt = vec![0u32; (w * h) as usize];
    let mut px = vec![0u32; n];
    for y in 0..height as usize {
        let r = &data[y * row..(y + 1) * row];
        let ty = y as u32 / f;
        for x in 0..width as usize {
            for (c, v) in px.iter_mut().enumerate() {
                let s = sample(r, x * n + c, bpc);
                let s = if flip[c] { max - s } else { s };
                // to 0..255 (Indexed keeps its index)
                *v = if matches!(space, Space::Indexed(..)) { s } else { s * 255 / max };
            }
            let g = space.grey(&px);
            let i = (ty * w + x as u32 / f) as usize;
            sum[i] += g as u32;
            cnt[i] += 1;
        }
    }
    let grey = sum.iter().zip(&cnt).map(|(s, c)| ((s + c / 2) / (*c).max(1)) as u8).collect();
    Ok(Thumb { width, height, w, h, grey })
}

/// A JPEG's samples, chosen by the frame's component count (its colour transform isn't known until
/// the scan starts): 1 component as grey; 3 as RGB (the decoder converts YCbCr, and passes RGB
/// through); 4 as CMYK, passed through (the decoder already undoes Adobe's inverted CMYK: on 003
/// the thumbnails match MuPDF's within a grey level), or for YCCK passed through and converted here
/// as libjpeg does. No YCCK JPEG has been checked against MuPDF yet; none occurs in the 003 sample.
fn jpeg(raw: &[u8]) -> Result<(Vec<u8>, Space, u32, u32), &'static str> {
    use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    let decode = |out: ColorSpace| -> Result<(Vec<u8>, u32, u32, u8), &'static str> {
        let mut dec = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(raw), DecoderOptions::default().jpeg_set_out_colorspace(out));
        let data = dec.decode().map_err(|_| "jpeg")?;
        let info = dec.info().ok_or("jpeg")?;
        Ok((data, info.width as u32, info.height as u32, info.components))
    };
    let mut head = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(raw), DecoderOptions::default());
    head.decode_headers().map_err(|_| "jpeg")?;
    let comps = head.info().ok_or("jpeg")?.components;
    let (data, w, h, space) = match comps {
        1 => { let (d, w, h, _) = decode(ColorSpace::Luma)?; (d, w, h, Space::Gray) }
        3 => { let (d, w, h, _) = decode(ColorSpace::RGB)?; (d, w, h, Space::Rgb) }
        4 => match decode(ColorSpace::CMYK) {
            Ok((d, w, h, _)) => (d, w, h, Space::Cmyk),
            Err(_) => {
                let (mut d, w, h, _) = decode(ColorSpace::YCCK)?;
                for p in d.chunks_mut(4) { ycc_to_cmy(p); }
                (d, w, h, Space::Cmyk)
            }
        },
        _ => return Err("jpeg"),
    };
    if data.len() < (w * h) as usize * space.components() { return Err("jpeg"); }
    Ok((data, space, w, h))
}

/// YCCK to CMYK in place, as libjpeg's ycck_cmyk_convert: Y, Cb, Cr to RGB by the JFIF formulas,
/// then C = 255 - R and so on; K stays.
fn ycc_to_cmy(p: &mut [u8]) {
    let (y, cb, cr) = (p[0] as i32, p[1] as i32 - 128, p[2] as i32 - 128);
    let clamp = |v: i32| v.clamp(0, 255);
    let r = clamp(y + ((91_881 * cr + 32_768) >> 16));
    let g = clamp(y - ((22_554 * cb + 46_802 * cr - 32_768) >> 16));
    let b = clamp(y + ((116_130 * cb + 32_768) >> 16));
    p[0] = (255 - r) as u8;
    p[1] = (255 - g) as u8;
    p[2] = (255 - b) as u8;
}

enum Space {
    Gray, Rgb, Cmyk,
    /// One tint: 0 no ink (white), full = solid ink.
    Tint,
    /// A stencil mask: sample 0 paints (black), 1 leaves the page (white), before /Decode.
    Mask,
    /// Base space and the lookup table, base components per entry.
    Indexed(Box<Space>, Vec<u8>),
}

impl Space {
    fn components(&self) -> usize {
        match self { Space::Rgb => 3, Space::Cmyk => 4, _ => 1 }
    }

    /// Grey level 0..255 from components already scaled to 0..255 (an Indexed sample keeps its index).
    fn grey(&self, v: &[u32]) -> u8 {
        match self {
            Space::Gray | Space::Mask => v[0].min(255) as u8,
            Space::Tint => (255 - v[0].min(255)) as u8,
            Space::Rgb => ((299 * v[0] + 587 * v[1] + 114 * v[2] + 500) / 1000).min(255) as u8,
            Space::Cmyk => {
                let k = v[3];
                let (r, g, b) = (255 - (v[0] + k).min(255), 255 - (v[1] + k).min(255), 255 - (v[2] + k).min(255));
                ((299 * r + 587 * g + 114 * b + 500) / 1000) as u8
            }
            Space::Indexed(base, table) => {
                let m = base.components();
                let i = v[0] as usize * m;
                if i + m > table.len() { return 255; }
                let e: Vec<u32> = table[i..i + m].iter().map(|&b| b as u32).collect();
                base.grey(&e)
            }
        }
    }
}

fn space_of(pdf: &Pdf, v: Option<Val>) -> Result<Space, &'static str> {
    match v.map(|v| pdf.direct(v)) {
        None => Err("no_colour_space"),
        Some(Val::Name(n)) => named(&n),
        Some(Val::Array(a)) => {
            let first = skip_ws(&a, 0);
            let fam = match parse_val(&a, first) { Val::Name(n) => n, _ => return Err("colour_space") };
            let second = skip_val(&a, first);
            match fam.as_slice() {
                b"CalGray" => Ok(Space::Gray),
                b"CalRGB" => Ok(Space::Rgb),
                b"ICCBased" => {
                    let icc = pdf.resolve(&parse_val(&a, second)).ok_or("colour_space")?;
                    match get(&icc, b"/N").map(|v| pdf.direct(v)) {
                        Some(Val::Num(x)) if x == 1.0 => Ok(Space::Gray),
                        Some(Val::Num(x)) if x == 3.0 => Ok(Space::Rgb),
                        Some(Val::Num(x)) if x == 4.0 => Ok(Space::Cmyk),
                        _ => Err("colour_space"),
                    }
                }
                b"Separation" => Ok(Space::Tint),
                b"Indexed" | b"I" => {
                    let base = space_of(pdf, Some(parse_val(&a, second)))?;
                    if matches!(base, Space::Indexed(..) | Space::Mask) { return Err("colour_space"); }
                    let third = skip_val(&a, second);
                    let fourth = skip_val(&a, third);
                    let table = match parse_val(&a, fourth) {
                        Val::Ref(r) => pdf.stream(r).or_else(|| pdf.dict(r).and_then(|s| string_bytes(&s))).ok_or("lookup")?,
                        _ => string_bytes(&a[skip_ws(&a, fourth)..]).ok_or("lookup")?,
                    };
                    Ok(Space::Indexed(Box::new(base), table))
                }
                _ => Err("colour_space"),
            }
        }
        _ => Err("colour_space"),
    }
}

fn named(n: &[u8]) -> Result<Space, &'static str> {
    match n {
        b"DeviceGray" | b"G" => Ok(Space::Gray),
        b"DeviceRGB" | b"RGB" => Ok(Space::Rgb),
        b"DeviceCMYK" | b"CMYK" => Ok(Space::Cmyk),
        _ => Err("colour_space"),
    }
}

/// A PDF string, literal "(...)" or hex "<...>", as bytes.
fn string_bytes(s: &[u8]) -> Option<Vec<u8>> {
    let i = skip_ws(s, 0);
    match s.get(i)? {
        b'<' => {
            let digits: Vec<u8> = s[i + 1..].iter().copied().take_while(|&b| b != b'>').filter(|b| b.is_ascii_hexdigit()).collect();
            let h = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
            Some(digits.chunks(2).map(|p| (h(p[0]) << 4) | if p.len() > 1 { h(p[1]) } else { 0 }).collect())
        }
        b'(' => {
            let (mut out, mut depth, mut j) = (Vec::new(), 0usize, i + 1);
            while j < s.len() {
                let c = s[j];
                match c {
                    b'\\' => {
                        j += 1;
                        let Some(&e) = s.get(j) else { break };
                        match e {
                            b'n' => out.push(b'\n'), b'r' => out.push(b'\r'), b't' => out.push(b'\t'),
                            b'b' => out.push(8), b'f' => out.push(12),
                            b'0'..=b'7' => {
                                let mut v = 0u32;
                                let mut k = 0;
                                while k < 3 && j < s.len() && (b'0'..=b'7').contains(&s[j]) { v = v * 8 + (s[j] - b'0') as u32; j += 1; k += 1; }
                                out.push(v as u8);
                                continue;
                            }
                            b'\r' | b'\n' => {}
                            other => out.push(other),
                        }
                    }
                    b'(' => { depth += 1; out.push(c); }
                    b')' => { if depth == 0 { break; } depth -= 1; out.push(c); }
                    _ => out.push(c),
                }
                j += 1;
            }
            Some(out)
        }
        _ => None,
    }
}

/// The decode parameters of the last filter (the one whose output is the samples).
fn last_parms(pdf: &Pdf, v: Option<Val>) -> Option<Vec<u8>> {
    match v.map(|v| pdf.direct(v)) {
        Some(Val::Dict(d)) => Some(d),
        Some(Val::Array(a)) => {
            let (mut last, mut i) = (None, 0);
            while i < a.len() {
                i = skip_ws(&a, i);
                if i >= a.len() { break; }
                last = match parse_val(&a, i) { Val::Dict(d) => Some(d), Val::Ref(r) => pdf.dict(r), _ => None };
                i = skip_val(&a, i).max(i + 1);
            }
            last
        }
        _ => None,
    }
}

fn nums(a: &[u8]) -> Vec<f64> {
    let (mut out, mut i) = (Vec::new(), 0);
    while i < a.len() {
        i = skip_ws(a, i);
        if i >= a.len() { break; }
        if let Val::Num(x) = parse_val(a, i) { out.push(x); }
        i = skip_val(a, i).max(i + 1);
    }
    out
}

/// Undo a PNG (10 to 15) or TIFF (2) predictor (ISO 32000-1 7.4.4.4). Without one the data is as is.
fn unpredict(pdf: &Pdf, parms: Option<&[u8]>, data: Vec<u8>, row: usize, n: usize, bpc: usize) -> Result<Vec<u8>, &'static str> {
    let Some(p) = parms else { return Ok(data) };
    let int = |k: &[u8], d: usize| match get(p, k).map(|v| pdf.direct(v)) { Some(Val::Num(x)) if x >= 0.0 => x as usize, _ => d };
    let pred = int(b"/Predictor", 1);
    if pred == 1 { return Ok(data); }
    let colors = int(b"/Colors", n);
    let bits = int(b"/BitsPerComponent", bpc);
    let columns = int(b"/Columns", 1);
    let prow = (colors * bits * columns + 7) / 8;
    let bpp = ((colors * bits + 7) / 8).max(1);
    if pred == 2 {
        if bits != 8 { return Err("predictor"); }
        let mut out = data;
        for r in out.chunks_mut(prow) {
            for i in bpp..r.len() { r[i] = r[i].wrapping_add(r[i - bpp]); }
        }
        return Ok(out);
    }
    if pred < 10 { return Err("predictor"); }
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; prow];
    for chunk in data.chunks(prow + 1) {
        if chunk.len() < prow + 1 { break; }
        let (t, src) = (chunk[0], &chunk[1..]);
        let mut cur = vec![0u8; prow];
        for i in 0..prow {
            let a = if i >= bpp { cur[i - bpp] as i32 } else { 0 };
            let b = prev[i] as i32;
            let c = if i >= bpp { prev[i - bpp] as i32 } else { 0 };
            let x = src[i];
            cur[i] = match t {
                0 => x,
                1 => x.wrapping_add(a as u8),
                2 => x.wrapping_add(b as u8),
                3 => x.wrapping_add(((a + b) / 2) as u8),
                4 => {
                    let pa = (b - c).abs();
                    let pb = (a - c).abs();
                    let pc = (a + b - 2 * c).abs();
                    x.wrapping_add(if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c } as u8)
                }
                _ => return Err("predictor"),
            };
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    // the predictor's rows can be wider than the image's (Columns set larger); keep the image's width
    if prow != row {
        let rows = out.len() / prow;
        let mut cut = Vec::with_capacity(rows * row);
        for r in 0..rows { cut.extend_from_slice(&out[r * prow..r * prow + row.min(prow)]); }
        return Ok(cut);
    }
    Ok(out)
}

/// Sample `k` of a row, `bpc` bits each (16-bit samples keep their top byte).
fn sample(row: &[u8], k: usize, bpc: u32) -> u32 {
    match bpc {
        8 => row[k] as u32,
        16 => row[2 * k] as u32,
        _ => {
            let bit = k * bpc as usize;
            let byte = row[bit / 8] as u32;
            let shift = 8 - bpc - (bit % 8) as u32;
            (byte >> shift) & ((1 << bpc) - 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf_with(image: &str, data: &[u8]) -> Vec<u8> {
        let mut s = format!("%PDF-1.4\n1 0 obj << /Type /XObject /Subtype /Image {} /Length {} >> stream\n", image, data.len()).into_bytes();
        s.extend_from_slice(data);
        s.extend_from_slice(b"\nendstream endobj\ntrailer << >>\n%%EOF\n");
        s
    }

    #[test]
    fn a_grey_image_shrinks_by_averaging() {
        // 4 x 2, 8-bit grey: left half black, right half white; 256 px limit keeps it as is
        // an inline image, short keys: the same pixels, plain and through /AHx and /Fl
        let i = inline_thumbnail(b"/W 4 /H 2 /CS /G /BPC 8", &[0, 0, 255, 255, 0, 0, 255, 255], 1).unwrap();
        assert!(i.width == 4 && i.height == 2 && i.w == 1 && i.h == 1 && i.grey == vec![128], "{:?}", (i.w, i.h, &i.grey));
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&[0, 255], 6);
        let hex: String = z.iter().map(|b| format!("{b:02X}")).collect::<String>() + ">";
        let i = inline_thumbnail(b"/W 2 /H 1 /CS /G /BPC 8 /F [/AHx /Fl]", hex.as_bytes(), 8).unwrap();
        assert_eq!(i.grey, vec![0, 255]);
        let t = image_thumbnail(&pdf_with("/Width 4 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8", &[0, 0, 255, 255, 0, 0, 255, 255]), 1).unwrap();
        assert_eq!((t.w, t.h), (4, 2));
        assert_eq!(t.grey, [0, 0, 255, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn rgb_indexed_masks_and_decode() {
        // RGB red -> luma 76
        let t = image_thumbnail(&pdf_with("/Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8", &[255, 0, 0]), 1).unwrap();
        assert_eq!(t.grey, [76]);
        // a stencil mask: bit 0 paints black, 1 leaves white; one byte holds 8 pixels
        let t = image_thumbnail(&pdf_with("/Width 8 /Height 1 /ImageMask true", &[0b0000_1111]), 1).unwrap();
        assert_eq!(t.grey, [0, 0, 0, 0, 255, 255, 255, 255]);
        // /Decode [1 0] flips a 1-bit grey image
        let t = image_thumbnail(&pdf_with("/Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 1 /Decode [1 0]", &[0b1000_0000]), 1).unwrap();
        assert_eq!(t.grey, [0, 255]);
        // Indexed over RGB with a hex lookup: index 1 is white
        let t = image_thumbnail(&pdf_with("/Width 2 /Height 1 /ColorSpace [/Indexed /DeviceRGB 1 <000000FFFFFF>] /BitsPerComponent 8", &[0, 1]), 1).unwrap();
        assert_eq!(t.grey, [0, 255]);
    }

    #[test]
    fn a_png_predictor_is_undone() {
        // 2 x 2 grey, rows filtered Sub then Up: row 1 = 10, 20; row 2 = 10, 20 again
        let t = image_thumbnail(&pdf_with("/Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /DecodeParms << /Predictor 15 /Colors 1 /Columns 2 >>",
            &[1, 10, 10, 2, 0, 0]), 1).unwrap();
        assert_eq!(t.grey, [10, 20, 10, 20]);
    }

    #[test]
    fn a_fax_image_decodes() {
        // Group 4, 8 x 1: horizontal mode, white 2, black 4, then V0 to the edge (see ccitt.rs)
        let t = image_thumbnail(&pdf_with("/Width 8 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 8 >>",
            &[0b0010_1110, 0b1110_0000]), 1).unwrap();
        assert_eq!(t.grey, [255, 255, 0, 0, 0, 0, 255, 255]);
    }

    #[test]
    fn strips_make_one_canvas() {
        // two objects, 4 x 1 each: a black strip over a white one, drawn as two halves of a box
        let mut s = b"%PDF-1.4\n".to_vec();
        for (n, v) in [(1u32, 0u8), (2, 255)] {
            s.extend_from_slice(format!("{} 0 obj << /Type /XObject /Subtype /Image /Width 4 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >> stream\n", n).as_bytes());
            s.extend_from_slice(&[v; 4]);
            s.extend_from_slice(b"\nendstream endobj\n");
        }
        s.extend_from_slice(b"trailer << >>\n%%EOF\n");
        let up = Turn::default();
        let piece = |obj, bx| crate::Piece { obj, bx, turn: up };
        let t = merged_thumbnail(&s, &[piece(1, [0.0, 0.0, 4.0, 1.0]), piece(2, [0.0, 1.0, 4.0, 2.0])], 256).unwrap();
        assert_eq!((t.w, t.h), (4, 2));
        assert_eq!(t.grey, [0, 0, 0, 0, 255, 255, 255, 255]);
        // the same strips on a page turned by /Rotate 90: stored 4 x 1, each shows as 1 x 4, side by
        // side, the first one drawn on the right (results/kinds-turn.md)
        let r90 = Turn { transpose: true, flip_x: true, flip_y: false };
        let piece = |obj, bx| crate::Piece { obj, bx, turn: r90 };
        let t = merged_thumbnail(&s, &[piece(1, [1.0, 0.0, 2.0, 4.0]), piece(2, [0.0, 0.0, 1.0, 4.0])], 256).unwrap();
        assert_eq!((t.w, t.h), (2, 4));
        assert_eq!(t.grey, [255, 0, 255, 0, 255, 0, 255, 0]);
    }

    #[test]
    fn turn_follows_the_page_rotation() {
        // an image filling a W x H page, its unit square's corners mapped as PageBox::map does
        let (w, h) = (2.0, 3.0);
        let map = |rot: u32, x: f64, y: f64| { let (u, v) = (x, h - y); match rot { 90 => (h - v, u), 180 => (w - u, h - v), 270 => (v, w - u), _ => (u, v) } };
        let turn = |rot| Turn::from_corners(map(rot, 0.0, h), map(rot, w, h), map(rot, 0.0, 0.0));
        assert_eq!(turn(0), Turn::default());
        assert_eq!(turn(90), Turn { transpose: true, flip_x: true, flip_y: false });
        assert_eq!(turn(180), Turn { transpose: false, flip_x: true, flip_y: true });
        assert_eq!(turn(270), Turn { transpose: true, flip_x: false, flip_y: true });
        // an image drawn upside down by its own matrix (negative height) on an upright page
        assert_eq!(Turn::from_corners((0.0, 3.0), (2.0, 3.0), (0.0, 0.0)), Turn { transpose: false, flip_x: false, flip_y: true });
        // a 2 x 1 thumbnail [a b] turned a quarter clockwise shows a over b
        let t = Thumb { width: 2, height: 1, w: 2, h: 1, grey: vec![10, 20] };
        let r = turn(90).apply(t);
        assert_eq!((r.w, r.h, r.width, r.height), (1, 2, 1, 2));
        assert_eq!(r.grey, [10, 20]);
    }

    #[test]
    fn filters_not_decoded_yet_say_so() {
        let e = image_thumbnail(&pdf_with("/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /JPXDecode", &[0]), 1).err();
        assert_eq!(e, Some("jpx"));
        // a broken JPEG says so rather than guessing
        let e = image_thumbnail(&pdf_with("/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode", &[0]), 1).err();
        assert_eq!(e, Some("jpeg"));
    }
}
