// Check the WebAssembly build (wasm/pkg, wasm-pack --target web) against native code, file by file:
//   - extract_json must equal regions-cli's output byte for byte (regions-cli adds file, bytes and
//     micros in front);
//   - extract_json and kinds_json must equal the same calls compiled natively (wasm/src/bin/native.rs)
//     byte for byte;
//   - kinds_json's vector kinds must equal the vkinds tool's.
// Also prints the build's size, and the time of extract_json a page in the browser build and natively
// (the same code and profile), as the median over files of each file's time over its pages.
// Works 40 files at a time (native tools, browser build, compare), so memory stays at one batch.
//
// usage: node tools/wasm_check.mjs <folder of PDFs>...   (progress goes to stderr)
import { readFileSync, readdirSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { gzipSync } from "node:zlib";
import { join, basename } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const wasmFile = join(ROOT, "wasm", "pkg", "regions_wasm_bg.wasm");
const { initSync, extract_json, kinds_json } = await import("file:///" + join(ROOT, "wasm", "pkg", "regions_wasm.js").split("\\").join("/"));
const wasmBytes = readFileSync(wasmFile);
initSync({ module: wasmBytes });

const CLI = join(ROOT, "regions", "target", "release", "regions-cli.exe");
const VKINDS = join(ROOT, "regions", "target", "release", "vkinds.exe");
const NATIVE = join(ROOT, "wasm", "target", "release", "native.exe");
const files = process.argv.slice(2).flatMap((d) => readdirSync(d).filter((f) => f.toLowerCase().endsWith(".pdf")).sort().map((f) => join(d, f)));
const tmp = mkdtempSync(join(tmpdir(), "wasmcheck-"));

// one native tool over one batch of files, its output lines keyed by file name
function native(exe, batch) {
  const out = new Map();
  const list = join(tmp, "list.txt");
  writeFileSync(list, batch.map((f) => f.split("\\").join("/")).join("\n"));
  const text = execFileSync(exe, ["--list", list], { maxBuffer: 1 << 30, encoding: "utf-8" });
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    const k = basename(JSON.parse(line.slice(0, line.indexOf(',"', 9)) + "}").file);
    if (!out.has(k)) out.set(k, []);
    out.get(k).push(line);
  }
  return out;
}

const median = (v) => { const s = [...v].sort((a, b) => a - b); return s.length ? s[Math.floor(s.length / 2)] : 0; };
let cliSame = 0, natMapSame = 0, natKindsSame = 0, vSame = 0, vDiff = 0, pages = 0, n = 0;
const bad = [], wasmPage = [], natPage = [];
for (let b = 0; b < files.length; b += 40) {
  const batch = files.slice(b, b + 40);
  const cli = native(CLI, batch), vk = native(VKINDS, batch), nat = native(NATIVE, batch);
  for (const f of batch) {
    const name = basename(f);
    const data = readFileSync(f);
    const t0 = process.hrtime.bigint();
    const wj = extract_json(data);
    const wmicros = Number(process.hrtime.bigint() - t0) / 1000;
    const wk = kinds_json(data);
    const line = cli.get(name)[0];
    if ("{" + line.slice(line.indexOf(",", line.indexOf('"micros"')) + 1) === wj) cliSame++; else bad.push(`${name} map vs regions-cli`);
    const nl = nat.get(name)[0];
    const nmicros = Number(nl.slice(nl.indexOf('"micros":') + 9, nl.indexOf(',"map"')));
    const nmap = nl.slice(nl.indexOf(',"map":') + 7, nl.lastIndexOf(',"kinds":'));
    const nkinds = nl.slice(nl.lastIndexOf(',"kinds":') + 9, -1);
    if (nmap === wj) natMapSame++; else bad.push(`${name} map vs native`);
    if (nkinds === wk) natKindsSame++; else bad.push(`${name} kinds vs native`);
    const pageCount = JSON.parse(wj).pages.length;
    pages += pageCount;
    wasmPage.push(wmicros / Math.max(pageCount, 1) / 1000);
    natPage.push(nmicros / Math.max(pageCount, 1) / 1000);
    const nv = new Map((vk.get(name) || []).map((l) => JSON.parse(l)).map((j) => [`${j.page}/${j.index}`, j]));
    JSON.parse(wk).pages.forEach((p, pno) => p.vectors.forEach((k, i) => {
      const v = nv.get(`${pno}/${i}`);
      if (v && v.kind === k.kind && v.confidence === k.confidence) vSame++; else vDiff++;
    }));
    n++;
  }
  process.stderr.write(`${n} of ${files.length} files\n`);
}
rmSync(tmp, { recursive: true, force: true });
console.log(`${files.length} files, ${pages} pages`);
console.log(`extract_json identical to regions-cli: ${cliSame} of ${files.length}`);
console.log(`extract_json identical to the native build: ${natMapSame} of ${files.length}; kinds_json: ${natKindsSame} of ${files.length}`);
console.log(`vector kinds identical to vkinds: ${vSame}, different: ${vDiff}`);
for (const x of bad.slice(0, 8)) console.log("  differs:", x);
console.log(`wasm size ${wasmBytes.length} bytes raw, ${gzipSync(wasmBytes, { level: 9 }).length} gzipped`);
console.log(`extract_json a page, median over files: wasm ${median(wasmPage).toFixed(2)} ms, native (same code, opt-level s) ${median(natPage).toFixed(2)} ms`);
process.exit(bad.length || vDiff ? 1 : 0);
