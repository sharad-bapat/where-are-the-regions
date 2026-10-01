"""Score the kind layer for vectors (regions::vkind, plans/chunk9-kind-vectors.md 9c) on tuning data.

Runs the vkinds tool and compares each vector cluster's kind with the truth:

  vector set   data/constructed/vectors (tools/build_vectors.py): each case draws one kind in a known
               area. The largest cluster inside the area is judged; for outlined text, the kind most
               of the area's clusters get (its letters are separate clusters).
  hand labels  a seeded sample of real clusters labelled by eye from contact sheets, 25 per
               predicted kind (data/real/tune-003-vectors.jsonl), matched by file, page and cluster
               index. "unclear" labels are counted apart.

Targets (D90): on the vector set at least 90% right overall, and table grids and outlined text at
least 90% found; calibration, at confidence 0.9 or more at least 90% right.

usage: python tools/score_vkinds.py [constructed] [real] [--heldout] [--misses]
--heldout scores the vector set's heldout split and the 004 labels (heldout-004-vectors.jsonl) and
is refused unless tools/check_frozen.py passes (9d).
"""
import json
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VKINDS = ROOT / "regions" / "target" / "release" / "vkinds.exe"
GOVDOCS = Path("C:/Users/sharad/Downloads/05_Research-Reference/Datasets/govdocs1")
WANT = {"rule": "rule", "border": "border", "table": "table_grid", "fill": "fill", "chart": "chart_or_diagram", "outlined": "outlined_text"}
BANDS = [(0.9, 1.01), (0.7, 0.9), (0.5, 0.7), (0.0, 0.5)]


def run(files):
    out = defaultdict(list)
    for i in range(0, len(files), 40):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
            f.write("\n".join(Path(p).resolve().as_posix() for p in files[i:i + 40]))
            lst = f.name
        res = subprocess.run([str(VKINDS), "--list", lst], capture_output=True, text=True, encoding="utf-8").stdout
        Path(lst).unlink()
        for line in res.splitlines():
            j = json.loads(line)
            out[Path(j["file"]).name].append(j)
    return out


def band_of(c):
    return next(b for b in BANDS if b[0] <= c < b[1])


def area(j):
    b = j["box"]
    return (b[2] - b[0]) * (b[3] - b[1])


def constructed(split, calib, misses):
    man = json.loads((ROOT / "data" / "constructed" / "vectors" / "manifest.json").read_text(encoding="utf-8"))
    items = [i for i in man["items"] if i["split"] == split]
    got = run([ROOT / "data" / "constructed" / "vectors" / i["file"] for i in items])
    table = defaultdict(Counter)
    right = Counter()
    miss = []
    for it in items:
        b = it["box"]
        cs = [j for j in got[Path(it["file"]).name] if j["box"][0] >= b[0] - 3 and j["box"][1] >= b[1] - 3 and j["box"][2] <= b[2] + 3 and j["box"][3] <= b[3] + 3]
        if not cs:
            table[it["kind"]]["(no cluster)"] += 1
            right[(it["kind"], False)] += 1
            continue
        if it["kind"] == "outlined":
            k = Counter(c["kind"] for c in cs).most_common(1)[0][0]
            conf = max(c["confidence"] for c in cs if c["kind"] == k)
        else:
            j = max(cs, key=area)
            k, conf = j["kind"], j["confidence"]
        ok = k == WANT[it["kind"]]
        table[it["kind"]][k] += 1
        right[(it["kind"], ok)] += 1
        calib[band_of(conf)][ok] += 1
        if not ok:
            miss.append((it["id"], it["kind"], it["note"], k, conf))
    total_ok = sum(v for (k, ok), v in right.items() if ok)
    total = sum(right.values())
    print(f"vector set {split}: kind by case kind")
    for kind in sorted(table):
        print(f"  {kind:9s} {dict(table[kind].most_common())}")
    print(f"  right overall: {total_ok}/{total} ({100 * total_ok / max(total, 1):.1f}%, target 90%)")
    for kind in ("table", "outlined"):
        ok, n = right[(kind, True)], right[(kind, True)] + right[(kind, False)]
        print(f"  {kind} found: {ok}/{n} ({100 * ok / max(n, 1):.1f}%, target 90%)")
    if misses:
        for m in miss:
            print("   miss", m)


def real(thread, labels_name, calib, misses):
    labs = [json.loads(l) for l in (ROOT / "data" / "real" / labels_name).read_text(encoding="utf-8").splitlines() if l.strip()]
    got = run([GOVDOCS / thread / f for f in sorted({l["file"] for l in labs})])
    by = {(Path(j["file"]).name, j["page"], j["index"]): j for js in got.values() for j in js}
    table = defaultdict(Counter)
    stratum = defaultdict(Counter)
    apart = Counter()
    miss = []
    for l in labs:
        if l["label"] == "unclear":
            apart["unclear"] += 1
            continue
        j = by.get((l["file"], l["page"], l["index"]))
        if j is None:
            apart["cluster not found"] += 1
            continue
        ok = j["kind"] == l["label"]
        table[l["label"]][j["kind"]] += 1
        stratum[l["sampled_as"]][ok] += 1
        calib[band_of(j["confidence"])][ok] += 1
        if not ok:
            miss.append((l["file"], l["page"], l["index"], l["label"], j["kind"], j["confidence"], j["features"]))
    ok = sum(c[True] for c in stratum.values())
    n = sum(c[True] + c[False] for c in stratum.values())
    print(f"govdocs1 {thread} hand labels: kind given, by true kind")
    for lab in sorted(table):
        c = table[lab]
        print(f"  {lab:17s} {c[lab]}/{sum(c.values())} right  {dict(c.most_common())}")
    print(f"  right overall: {ok}/{n} ({100 * ok / max(n, 1):.1f}%); by sampled stratum: "
          + ", ".join(f"{k} {c[True]}/{c[True] + c[False]}" for k, c in sorted(stratum.items())))
    print(f"  counted apart: {dict(apart)}")
    if misses:
        for m in miss:
            print("   miss", m)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    misses = "--misses" in sys.argv
    held = "--heldout" in sys.argv
    if held:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        from check_frozen import require_frozen
        require_frozen()
    calib = defaultdict(Counter)
    if not args or "constructed" in args:
        constructed("heldout" if held else "tune", calib, misses)
    if not args or "real" in args:
        real("004" if held else "003", "heldout-004-vectors.jsonl" if held else "tune-003-vectors.jsonl", calib, misses)
    print("calibration (share right by confidence band; target: 0.9 and up at least 90%):")
    for b in BANDS:
        c = calib[b]
        n = c[True] + c[False]
        print(f"  {b[0]:.1f} to {min(b[1], 1.0):.1f}: {c[True]}/{n}" + (f" ({100 * c[True] / n:.1f}%)" if n else ""))


if __name__ == "__main__":
    main()
