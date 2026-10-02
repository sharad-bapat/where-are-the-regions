//! The browser build's two calls run natively, one JSON line per file: {"file":..,"micros":..,"map":<extract_json>,"kinds":<kinds_json>}.
//! micros times extract_json alone, on bytes in memory, for comparing with the browser build.
//! usage: native [--list files.txt] [file.pdf...]
use std::time::Instant;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut paths = Vec::new();
    if args.first().map(|a| a == "--list").unwrap_or(false) {
        if let Some(list) = args.get(1).and_then(|p| std::fs::read_to_string(p).ok()) {
            paths.extend(list.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()));
        }
        args.drain(0..2.min(args.len()));
    }
    paths.extend(args);
    for path in paths {
        let Ok(data) = std::fs::read(&path) else { continue };
        let t = Instant::now();
        let map = regions_wasm::extract_json(&data);
        let micros = t.elapsed().as_secs_f64() * 1e6;
        println!("{{\"file\":{},\"micros\":{:.1},\"map\":{},\"kinds\":{}}}", regions::json_str(&path), micros, map, regions_wasm::kinds_json(&data));
    }
}
