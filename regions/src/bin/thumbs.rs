//! Checking tool for regions::pixels and regions::kind: every image the map places, inline ones keyed -1, -2...
//! usage: thumbs <out dir | -> [--kinds] [--list files.txt] [file.pdf...]
//! Writes <out dir>/<file stem>_<obj>.pgm for each image it can decode (not with "-") and prints one
//! JSON line per image: file, obj, its size, the thumbnail's size, or the reason it wasn't decoded.
//! With --kinds each line also has the image's kind, confidence, reasons and features (the kind is
//! judged on a thumbnail of up to kind::KIND_THUMB px, so the PGM is that size too).
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() { eprintln!("usage: thumbs <out dir | -> [--kinds] [--list files.txt] [file.pdf...]"); return; }
    let out = args.remove(0);
    let write = out != "-";
    if write { std::fs::create_dir_all(&out).ok(); }
    let mut kinds = false;
    let mut paths = Vec::new();
    while let Some(a) = args.first().cloned() {
        if a == "--kinds" {
            kinds = true;
            args.remove(0);
        } else if a == "--list" {
            if let Some(list) = args.get(1).and_then(|p| std::fs::read_to_string(p).ok()) {
                paths.extend(list.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()));
            }
            args.drain(0..2.min(args.len()));
        } else { break; }
    }
    paths.extend(args);
    let max = if kinds { regions::kind::KIND_THUMB } else { regions::pixels::THUMB };
    for path in paths {
        let Ok(data) = std::fs::read(&path) else { continue };
        let doc = regions::extract(&data);
        let stem = Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        // each image region once; a merged one (strips) is judged from all its pieces, and its line
        // is printed for every piece's object, so a label on any strip finds it
        // an inline image has no object number: it's keyed -1, -2... in drawing order within the file
        let mut seen: BTreeSet<i64> = BTreeSet::new();
        let mut inline_n = 0i64;
        for img in doc.pages.iter().flat_map(|p| p.images.iter()) {
            let (objs, result): (Vec<i64>, _) = match &img.inline_src {
                Some(src) => {
                    inline_n += 1;
                    (vec![-inline_n], regions::pixels::inline_thumbnail(&src.0, &src.1, max))
                }
                None => {
                    let objs: Vec<u32> = img.pieces.iter().map(|p| p.0).filter(|&o| o != 0).collect();
                    if objs.is_empty() || objs.iter().all(|&o| seen.contains(&(o as i64))) { continue; }
                    let r = if img.pieces.len() > 1 {
                        regions::pixels::merged_thumbnail(&data, &img.pieces, max)
                    } else {
                        regions::pixels::image_thumbnail_max(&data, objs[0], max)
                    };
                    (objs.iter().map(|&o| o as i64).collect(), r)
                }
            };
            for &obj in &objs {
                if !seen.insert(obj) { continue; }
            match &result {
                Ok(t) => {
                    if write {
                        let name = format!("{}/{}_{}.pgm", out, stem, obj);
                        if let Ok(mut f) = std::fs::File::create(&name) {
                            let _ = write!(f, "P5\n{} {}\n255\n", t.w, t.h);
                            let _ = f.write_all(&t.grey);
                        }
                    }
                    let mut line = format!("{{\"file\":{},\"obj\":{},\"image\":{},\"parts\":{},\"width\":{},\"height\":{},\"w\":{},\"h\":{}", regions::json_str(&path), obj, objs[0], img.pieces.len(), t.width, t.height, t.w, t.h);
                    if kinds {
                        let k = regions::kind::classify(t.w, t.h, &t.grey);
                        let f = &k.features;
                        let reasons: Vec<String> = k.reasons.iter().map(|(n, v)| format!("[\"{}\",{:.3}]", n, v)).collect();
                        let tr: Vec<String> = k.has_text_reasons.iter().map(|(n, v)| format!("[\"{}\",{:.3}]", n, v)).collect();
                        line += &format!(",\"kind\":\"{}\",\"confidence\":{},\"reasons\":[{}],\"has_text\":{},\"has_text_reasons\":[{}],\"features\":{{\"spread\":{:.2},\"levels\":{},\"extremes\":{:.4},\"ink\":{:.4},\"inverted\":{},\"components\":{},\"glyphs\":{},\"glyph_height\":{:.1},\"height_spread\":{:.3},\"aligned\":{:.3},\"runs\":{},\"word_marks\":{}}}",
                            k.kind, k.confidence, reasons.join(","), k.has_text, tr.join(","), f.spread, f.levels, f.extremes, f.ink, f.inverted, f.components, f.glyphs, f.glyph_height, f.height_spread, f.aligned, f.runs, f.word_marks);
                    }
                    println!("{}}}", line);
                }
                Err(e) => println!("{{\"file\":{},\"obj\":{},\"error\":\"{}\"}}", regions::json_str(&path), obj, e),
            }
            }
        }
    }
}
