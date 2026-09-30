//! Checking tool for regions::vkind: the kind of every vector cluster the map finds.
//! usage: vkinds [--list files.txt] [file.pdf...]
//! One JSON line per cluster: file, page, index (into the page's vector clusters), box, kind,
//! confidence, has_text, reasons and features.

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
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
        for (pno, page) in doc.pages.iter().enumerate() {
            let kinds = regions::vkind::classify_page(&page.vectors, &page.paths);
            for (i, (v, k)) in page.vectors.iter().zip(kinds.iter()).enumerate() {
                let f = &k.features;
                let reasons: Vec<String> = k.reasons.iter().map(|(n, x)| format!("[\"{}\",{:.3}]", n, x)).collect();
                println!("{{\"file\":{},\"page\":{},\"index\":{},\"box\":[{:.1},{:.1},{:.1},{:.1}],\"kind\":\"{}\",\"confidence\":{},\"has_text\":{},\"reasons\":[{}],\"features\":{{\"paths\":{},\"hlines\":{},\"vlines\":{},\"rows\":{},\"cols\":{},\"rects\":{},\"open_rects\":{},\"rect_cols\":{},\"rect_rows\":{},\"rect_cover\":{:.3},\"filled\":{:.3},\"curved\":{},\"glyphs\":{},\"w\":{:.1},\"h\":{:.1}}}}}",
                    regions::json_str(&path), pno, i, v.x0, v.y0, v.x1, v.y1, k.kind, k.confidence, k.has_text, reasons.join(","),
                    f.paths, f.hlines, f.vlines, f.rows, f.cols, f.rects, f.open_rects, f.rect_cols, f.rect_rows, f.rect_cover, f.filled, f.curved, f.glyphs, f.w, f.h);
            }
        }
    }
}
