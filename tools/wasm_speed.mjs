// Speed of the browser build (wasm/pkg) in Node: extract_json over every file in the given folders,
// several passes, each file's median over the passes. Prints the same measures as tools/speed.py.
// Keeps only the numbers. Run on a quiet machine, in the same session as speed.py.
//
// usage: node tools/wasm_speed.mjs <folder of PDFs>...   (3 passes; a progress bar goes to stderr)
import { readFileSync, readdirSync } from "node:fs";
import { join, basename } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const { initSync, extract_json } = await import("file:///" + join(ROOT, "wasm", "pkg", "regions_wasm.js").split("\\").join("/"));
initSync({ module: readFileSync(join(ROOT, "wasm", "pkg", "regions_wasm_bg.wasm")) });
const PASSES = 3;
const median = (v) => { const s = [...v].sort((a, b) => a - b); return s[Math.floor(s.length / 2)]; };
const pct = (v, q) => { const s = [...v].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.floor(q * s.length))]; };

// a one-line progress bar on stderr, like rich's: label, bar, percent, done/total, elapsed, time left
function bar(label, total) {
  const start = Date.now();
  const clock = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
  return {
    tick(done) {
      if (!process.stderr.isTTY) return;
      const el = (Date.now() - start) / 1000, frac = done / total, w = 30, fill = Math.round(frac * w);
      const left = done ? (el / done) * (total - done) : 0;
      process.stderr.write(`\r${label} ${"\u2501".repeat(fill)}${"\u2500".repeat(w - fill)} ${String(Math.round(frac * 100)).padStart(3)}% ${done}/${total}  elapsed ${clock(el)}  left ${clock(left)} `);
    },
    end() { if (process.stderr.isTTY) process.stderr.write("\r\x1b[2K"); },
  };
}

for (const folder of process.argv.slice(2)) {
  const files = readdirSync(folder).filter((f) => f.toLowerCase().endsWith(".pdf")).sort();
  const times = new Map(), pages = new Map();
  for (let k = 0; k < PASSES; k++) {
    const b = bar(`${basename(folder)} pass ${k + 1}/${PASSES}`, files.length);
    let done = 0;
    for (const f of files) {
      const data = readFileSync(join(folder, f));
      const t0 = process.hrtime.bigint();
      const json = extract_json(data);
      const ms = Number(process.hrtime.bigint() - t0) / 1e6;
      if (!times.has(f)) { times.set(f, []); pages.set(f, Math.max(JSON.parse(json).pages?.length || 0, 1)); }
      times.get(f).push(ms);
      b.tick(++done);
    }
    b.end();
  }
  const fileMs = new Map([...times].map(([f, v]) => [f, median(v)]));
  const perPage = [...fileMs].map(([f, ms]) => ms / pages.get(f));
  const total = [...fileMs.values()].reduce((a, b) => a + b, 0);
  const allPages = [...pages.values()].reduce((a, b) => a + b, 0);
  console.log(`${basename(folder)} (wasm): ${files.length} files, ${allPages} pages, ${PASSES} passes`);
  console.log(`  a page: median ${median(perPage).toFixed(2)} ms, 90th percentile ${pct(perPage, 0.9).toFixed(2)} ms`);
  console.log(`  a file: median ${median([...fileMs.values()]).toFixed(2)} ms`);
  console.log(`  all files: ${(total / 1000).toFixed(2)} s, ${(total / allPages).toFixed(2)} ms a page`);
}
