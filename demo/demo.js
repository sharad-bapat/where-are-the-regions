// Where are the regions? The page, drawn by pdf.js, with every region the map finds boxed on top of it
// (text lines, images, drawings and annotations, one colour each), and the regions listed beside it in
// drawing order with their kind and flags. Hover a box or a list item to light up both. Everything runs
// in this page: the file is never uploaded.
import init, { extract_json, kinds_json } from './regions_wasm.js';

// pdf.js and the WebAssembly module load on first use, not with the page, so the page itself stays light
let pdfjs = null;
async function loadPdfjs() {
  if (!pdfjs) {
    const lib = await import('./pdfjs/pdf.min.mjs');
    lib.GlobalWorkerOptions.workerSrc = new URL('./pdfjs/pdf.worker.min.mjs', import.meta.url).href;
    pdfjs = lib;
  }
  return pdfjs;
}

const root = document.getElementById('demo');
const el = (tag, attrs = {}, ...kids) => {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k.startsWith('on')) n.addEventListener(k.slice(2), v);
    else if (v !== false && v != null) n.setAttribute(k, v === true ? '' : v);
  }
  n.append(...kids.flat().filter((k) => k != null && k !== false));
  return n;
};

const SAMPLES = [['report.pdf', 'Report'], ['scan.pdf', 'Scan'], ['hidden.pdf', 'Hidden marks'], ['slide.pdf', 'Slide']];
const LAYERS = [['text', 'Text'], ['image', 'Images'], ['vector', 'Drawings'], ['annot', 'Annotations']];
const WHAT = { text: 'Text', image: 'Image', vector: 'Drawing', annot: 'Annotation' };
const KIND = {
  text: 'text image', photo: 'photo', graphic: 'graphic', blank: 'blank',
  rule: 'rule', border: 'border', table_grid: 'table grid', fill: 'fill', chart_or_diagram: 'chart or diagram', outlined_text: 'outlined text',
};
// flags that mean a reader won't see the mark, in words
const UNSEEN = {
  hidden: 'clipped away', offpage: 'off the page', white: 'white', invisible: 'invisible text',
  empty: 'paints nothing', no_appearance: 'nothing drawn',
};
const NOTED = { clipped: 'partly clipped', strips: 'drawn in strips', undecodable: 'no text in the file', shading: 'shading', mask: 'mask', inline: 'inline image' };

const input = el('input', { type: 'file', id: 'rg-file', accept: 'application/pdf,.pdf', class: 'sr-only' });
const zone = el('div', { class: 'rg-drop' },
  el('p', {}, 'Drop a PDF here, choose one, or try a sample.'),
  el('div', { class: 'rg-actions' },
    el('label', { for: 'rg-file', class: 'rg-btn' }, 'Choose a PDF'), input,
    SAMPLES.map(([f, label]) => el('button', { type: 'button', class: 'rg-btn ghost', onclick: () => sample(f, label) }, label))));
const status = el('p', { class: 'rg-status', 'aria-live': 'polite' });
const bar = el('div', { class: 'rg-bar', hidden: true });
const stage = el('div', { class: 'rg-stage' });
const list = el('ol', { class: 'rg-list', 'aria-label': 'Regions on this page, in drawing order' });
const view = el('div', { class: 'rg-view', hidden: true }, stage, list);
root.replaceChildren(zone, status, bar, view,
  el('p', { class: 'rg-note' }, 'Dashed boxes are marks a reader can’t see. Image kinds are judged from the pixels; drawing kinds are experimental. Your file stays on your device: it’s read in this page and never uploaded.'));

let ready = null;
let result = null, kinds = null, pdf = null, pdfReady = false, index = 0, renderTask = null;
const shown = { text: true, image: true, vector: true, annot: true };

async function load(fileName, bytes) {
  status.textContent = 'Reading…';
  await (ready ||= init());
  const t = performance.now();
  result = JSON.parse(extract_json(bytes));
  const ms = performance.now() - t;
  kinds = null;
  if (result.status !== 'ok') {
    bar.hidden = true; view.hidden = true;
    status.textContent = result.status === 'not_pdf' ? 'That isn’t a PDF.' : 'This PDF is encrypted with a password, or with a method this tool doesn’t support.';
    return;
  }
  const total = result.pages.reduce((n, p) => n + p.map.length, 0);
  const took = ms < 0.1 ? 'under 0.1' : ms < 10 ? ms.toFixed(1) : Math.round(ms);
  status.textContent = `${fileName}: ${result.pages.length} page${result.pages.length === 1 ? '' : 's'}, ${total.toLocaleString()} regions, mapped in ${took} ms in your browser.`;
  if (pdf) pdf.destroy().catch(() => {});
  pdf = null;
  pdfReady = false;
  index = 0;
  show();
  const mine = result;
  // the kinds decode every image, so they come after the map is on screen
  setTimeout(() => {
    if (result !== mine) return;
    try { kinds = JSON.parse(kinds_json(bytes)); } catch { kinds = null; }
    if (result !== mine) return;
    listRegions(result.pages[index]);
    const byId = new Map(result.pages[index].map.map((r) => [String(r.id), r]));
    for (const b of stage.querySelectorAll('.rg-box')) { const r = byId.get(b.dataset.k); if (r) b.title = label(r); }
  }, 30);
  loadPdfjs().then((lib) => {
    if (result !== mine) return; // another file was chosen meanwhile
    const task = lib.getDocument({ data: bytes.slice(), isEvalSupported: false });
    pdf = task;
    task.promise.then(() => { if (pdf === task) { pdfReady = true; drawPage(result.pages[index]); } }, () => { if (pdf === task) pdf = null; });
  }, () => { /* without pdf.js the boxes and the list still show */ });
}

const unseen = (r) => r.flags.some((f) => f in UNSEEN);

async function show() {
  const page = result.pages[index];
  bar.hidden = false; view.hidden = false;
  const n = (w) => page.map.filter((r) => r.what === w).length;
  const count = (k, one, many) => `${n(k)} ${n(k) === 1 ? one : many}`;
  const counts = [count('text', 'text line', 'text lines'), count('image', 'image', 'images'), count('vector', 'drawing', 'drawings'), count('annot', 'annotation', 'annotations')];
  const hid = page.map.filter(unseen).length;
  if (hid) counts.push(`${hid} you can’t see`);
  bar.replaceChildren(
    el('button', { type: 'button', class: 'rg-btn ghost', disabled: index === 0, onclick: () => go(-1), 'aria-label': 'Previous page' }, '‹'),
    el('span', { class: 'rg-page' }, `Page ${index + 1} of ${result.pages.length}`),
    el('button', { type: 'button', class: 'rg-btn ghost', disabled: index === result.pages.length - 1, onclick: () => go(1), 'aria-label': 'Next page' }, '›'),
    el('span', { class: 'rg-counts' }, counts.join(' · ')),
    el('span', { class: 'rg-toggles' }, LAYERS.map(([k, label]) => el('label', { class: 'rg-toggle' },
      el('input', { type: 'checkbox', checked: shown[k], onchange: (e) => { shown[k] = e.target.checked; stage.classList.toggle(`hide-${k}`, !shown[k]); } }),
      el('span', { class: `rg-swatch k-${k}`, style: 'background: rgb(var(--c))' }), ` ${label}`))));
  listRegions(page);
  await drawPage(page);
}

function go(d) { index = Math.max(0, Math.min(result.pages.length - 1, index + d)); show(); }

async function drawPage(page) {
  const width = Math.max(240, stage.clientWidth || 600);
  const scale = width / page.width;
  const canvas = el('canvas', { 'aria-hidden': 'true', hidden: true });
  const overlay = el('div', { class: 'rg-overlay' });
  stage.replaceChildren(canvas, overlay);
  stage.style.height = `${page.height * scale}px`;
  for (const [k] of LAYERS) stage.classList.toggle(`hide-${k}`, !shown[k]);
  const want = index;
  for (const r of page.map) {
    if (r.flags.includes('offpage')) continue;
    overlay.append(el('div', {
      class: `rg-box k-${r.what}${unseen(r) ? ' unseen' : ''}`,
      'data-k': r.id, title: label(r),
      style: `left:${r.x0 * scale}px;top:${r.y0 * scale}px;width:${Math.max(1, (r.x1 - r.x0) * scale)}px;height:${Math.max(1, (r.y1 - r.y0) * scale)}px`,
    }));
  }
  // draw the page only when pdf.js has the document; until then the boxes stand alone
  if (pdf && pdfReady) {
    try {
      const doc = await pdf.promise;
      if (want !== index) return;
      canvas.hidden = false;
      const p = await doc.getPage(index + 1);
      const vp = p.getViewport({ scale });
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.floor(vp.width * dpr);
      canvas.height = Math.floor(vp.height * dpr);
      canvas.style.width = `${vp.width}px`;
      canvas.style.height = `${vp.height}px`;
      if (renderTask) renderTask.cancel();
      renderTask = p.render({ canvasContext: canvas.getContext('2d'), viewport: vp, transform: dpr !== 1 ? [dpr, 0, 0, dpr, 0, 0] : null, annotationMode: pdfjs.AnnotationMode.ENABLE });
      await renderTask.promise.catch(() => {});
      stage.dataset.rendered = String(want + 1); // the page number just drawn, for tools/demo_check.mjs
    } catch { /* the boxes and the list still show without the drawing */ }
  }
}

// the kind the layer gives a region, when it has one: images and drawings, from kinds_json
function kindOf(r) {
  const p = kinds?.pages?.[index];
  const k = r.what === 'image' ? p?.images?.[r.image] : r.what === 'vector' ? p?.vectors?.[r.vector] : null;
  if (!k) return null;
  return `${KIND[k.kind] || k.kind}, ${Math.round(k.confidence * 100)}% sure${r.what === 'vector' ? ' (experimental)' : ''}`;
}

function label(r) {
  const parts = [WHAT[r.what] || r.what];
  const k = kindOf(r);
  if (k) parts.push(k);
  if (r.what === 'annot' && r.subtype) parts.push(r.subtype);
  const f = r.flags.map((x) => UNSEEN[x] || NOTED[x]).filter(Boolean);
  if (f.length) parts.push(f.join(', '));
  return parts.join(' · ');
}

function listRegions(page) {
  list.replaceChildren(...page.map.map((r) => {
    const k = kindOf(r);
    const f = r.flags.map((x) => UNSEEN[x] || NOTED[x]).filter(Boolean);
    return el('li', { class: `rg-item k-${r.what}`, 'data-k': r.id },
      el('span', { class: 'what' }, WHAT[r.what] || r.what),
      k && el('span', { class: 'kind' }, ` · ${k}`),
      r.what === 'annot' && r.subtype && el('span', { class: 'kind' }, ` · ${r.subtype}`),
      f.length > 0 && el('span', { class: 'flags' }, ` · ${f.join(', ')}`),
      r.what === 'text' && r.t && el('span', { class: 't' }, r.t));
  }));
  if (!page.map.length) list.replaceChildren(el('li', { class: 'rg-empty' }, 'Nothing is drawn on this page.'));
}

// hover either side lights up both
function light(k, on) {
  for (const n of root.querySelectorAll(`[data-k="${k}"]`)) n.classList.toggle('hot', on);
  if (on) root.querySelector(`.rg-list [data-k="${k}"]`)?.scrollIntoView({ block: 'nearest' });
}
for (const pane of [stage, list]) {
  pane.addEventListener('pointerover', (e) => { const k = e.target.closest('[data-k]')?.dataset.k; if (k) light(k, true); });
  pane.addEventListener('pointerout', (e) => { const k = e.target.closest('[data-k]')?.dataset.k; if (k) light(k, false); });
}

async function sample(file, name) {
  const res = await fetch(new URL(`samples/${file}`, import.meta.url));
  load(`Sample: ${name}`, new Uint8Array(await res.arrayBuffer()));
}

input.addEventListener('change', async () => {
  const f = input.files?.[0];
  if (f) load(f.name, new Uint8Array(await f.arrayBuffer()));
});
zone.addEventListener('dragover', (e) => { e.preventDefault(); zone.classList.add('over'); });
zone.addEventListener('dragleave', () => zone.classList.remove('over'));
zone.addEventListener('drop', async (e) => {
  e.preventDefault();
  zone.classList.remove('over');
  const f = e.dataTransfer?.files?.[0];
  if (f) load(f.name, new Uint8Array(await f.arrayBuffer()));
});
// #sample=report (or scan, hidden, slide) opens a sample straight away
const fromHash = () => {
  const s = new URLSearchParams(location.hash.slice(1)).get('sample');
  const hit = SAMPLES.find(([f]) => f === `${s}.pdf`);
  if (hit) sample(...hit);
};
window.addEventListener('hashchange', fromHash);
fromHash();

let resizeTimer;
window.addEventListener('resize', () => { clearTimeout(resizeTimer); resizeTimer = setTimeout(() => { if (result && !view.hidden) drawPage(result.pages[index]); }, 150); });
