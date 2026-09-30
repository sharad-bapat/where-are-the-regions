//! Checking tool for regions::pixels: every image XObject the map places, as a grey thumbnail.
//! usage: thumbs <out dir> [--list files.txt] [file.pdf...]
//! Writes <out dir>/<file stem>_<obj>.pgm for each image it can decode and prints one JSON line per
//! image: file, obj, its size, the thumbnail's size, or the reason it wasn't decoded.
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() { eprintln!("usage: thumbs <out dir> [--list files.txt] [file.pdf...]"); return; }
    let out = args.remove(0);
    std::fs::create_dir_all(&out).ok();
    let mut paths = Vec::new();
    while let Some(a) = args.first().cloned() {
        if a == "--list" {
            if let Some(list) = args.get(1).and_then(|p| std::fs::read_to_string(p).ok()) {
                paths.extend(list.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()));
            }
            args.drain(0..2.min(args.len()));
        } else { break; }
    }
    paths.extend(args);
    for path in paths {
        let Ok(data) = std::fs::read(&path) else { continue };
        let doc = regions::extract(&data);
        let objs: BTreeSet<u32> = doc.pages.iter().flat_map(|p| p.images.iter().map(|i| i.obj)).filter(|&o| o != 0).collect();
        let stem = Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        for obj in objs {
            match regions::pixels::image_thumbnail(&data, obj) {
                Ok(t) => {
                    let name = format!("{}/{}_{}.pgm", out, stem, obj);
                    if let Ok(mut f) = std::fs::File::create(&name) {
                        let _ = write!(f, "P5\n{} {}\n255\n", t.w, t.h);
                        let _ = f.write_all(&t.grey);
                    }
                    println!("{{\"file\":{},\"obj\":{},\"width\":{},\"height\":{},\"w\":{},\"h\":{}}}", regions::json_str(&path), obj, t.width, t.height, t.w, t.h);
                }
                Err(e) => println!("{{\"file\":{},\"obj\":{},\"error\":\"{}\"}}", regions::json_str(&path), obj, e),
            }
        }
    }
}
