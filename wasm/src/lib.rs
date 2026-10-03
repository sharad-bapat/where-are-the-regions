//! The browser build of Where are the regions?: the page map as JSON, the same as regions-cli prints
//! (regions::Doc::to_json), and the kind layer for its images and vector clusters.
use wasm_bindgen::prelude::*;

/// The page map for the bytes of one PDF: per page, its words, images, paths, annotations and the
/// map of every region in drawing order, with flags.
#[wasm_bindgen]
pub fn extract_json(bytes: &[u8]) -> String { regions::extract(bytes).to_json(false) }

/// The kind layer for the same PDF, per page: one entry per image and per vector cluster, in the
/// order the map's "image" and "vector" indexes count them. An image the decoders can't read (or an
/// inline image, which has no object) gets null. Vector kinds are experimental (results/vkind-heldout.md).
#[wasm_bindgen]
pub fn kinds_json(bytes: &[u8]) -> String {
    let doc = regions::extract(bytes);
    let pages: Vec<String> = doc.pages.iter().map(|p| {
        let images: Vec<String> = p.images.iter().map(|img| {
            let objs: Vec<u32> = img.pieces.iter().map(|q| q.0).filter(|&o| o != 0).collect();
            let t = if let Some(src) = &img.inline_src {
                regions::pixels::inline_thumbnail(&src.0, &src.1, regions::kind::KIND_THUMB)
            } else if objs.is_empty() {
                return "null".into();
            } else if img.pieces.len() > 1 {
                regions::pixels::merged_thumbnail(bytes, &img.pieces, regions::kind::KIND_THUMB)
            } else {
                regions::pixels::image_thumbnail_max(bytes, objs[0], regions::kind::KIND_THUMB)
            };
            match t {
                Ok(t) => {
                    let k = regions::kind::classify(t.w, t.h, &t.grey);
                    format!("{{\"kind\":\"{}\",\"confidence\":{},\"has_text\":{}}}", k.kind, k.confidence, k.has_text)
                }
                Err(_) => "null".into(),
            }
        }).collect();
        let vectors: Vec<String> = regions::vkind::classify_page(&p.vectors, &p.paths).iter()
            .map(|k| format!("{{\"kind\":\"{}\",\"confidence\":{},\"has_text\":{}}}", k.kind, k.confidence, k.has_text))
            .collect();
        format!("{{\"images\":[{}],\"vectors\":[{}]}}", images.join(","), vectors.join(","))
    }).collect();
    format!("{{\"pages\":[{}]}}", pages.join(","))
}
