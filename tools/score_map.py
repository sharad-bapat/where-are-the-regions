"""Score the page map against the marks set (tools/build_marks.py), whose boxes are known exactly.

For each mark, the map region of the same kind ("text", "image", "vector", "annot") with the
closest box is its match, each region matched at most once:

  found      a match with every edge within TOL_PT of the mark's box
  flags      the match carries every flag the mark expects (invisible, white, clipped, hidden, ...)
  extra      map regions that match no mark

Marks are grouped by kind and their expected flags. Hidden and offpage marks are matched by box
too: the map keeps their drawn box.

usage: python tools/score_map.py [split=tune] [--misses]
"""
import json
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT / "regions" / "target" / "release" / "regions-cli.exe"
MARKS = ROOT / "data" / "constructed" / "marks"
TOL_PT = 1.0


def run(files):
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
        f.write("\n".join(str(p.resolve()).replace("\\", "/") for p in files))
        lst = f.name
    out = subprocess.run([str(CLI), "--list", lst], capture_output=True, text=True, encoding="utf-8", check=True).stdout
    Path(lst).unlink()
    return {Path(d["file"]).name: d for d in map(json.loads, out.splitlines())}


def err(a, b):
    return max(abs(a[0] - b["x0"]), abs(a[1] - b["y0"]), abs(a[2] - b["x1"]), abs(a[3] - b["y1"]))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    split = args[0] if args else "tune"
    if split != "tune":
        sys.exit("only the tune split is scored until the rules are frozen")
    show = "--misses" in sys.argv
    man = json.loads((MARKS / "manifest.json").read_text(encoding="utf-8"))
    items = [it for it in man["items"] if it["split"] == split]
    maps = run([MARKS / it["file"] for it in items])

    groups = defaultdict(lambda: {"marks": 0, "found": 0, "flags_ok": 0, "worst": 0.0, "errs": []})
    extra = defaultdict(int)
    misses = []
    for it in items:
        page = maps[Path(it["file"]).name]["pages"][0]
        entries = page["map"]
        used = set()
        # closest pairs first, so a near miss can't take another mark's region
        pairs = sorted((err(m["box"], e), mi, ei) for mi, m in enumerate(it["marks"])
                       for ei, e in enumerate(entries) if e["what"] == m["what"])
        match = {}
        for d, mi, ei in pairs:
            if mi in match or ei in used:
                continue
            match[mi] = (ei, d)
            used.add(ei)
        for mi, m in enumerate(it["marks"]):
            g = groups[(m["what"], ",".join(m["flags"]))]
            g["marks"] += 1
            if mi not in match:
                misses.append((it["id"], m["what"], m["flags"], m["box"], "no region of this kind"))
                continue
            ei, d = match[mi]
            e = entries[ei]
            g["errs"].append(d)
            if d <= TOL_PT:
                g["found"] += 1
                g["worst"] = max(g["worst"], d)
            if set(m["flags"]) <= set(e["flags"]):
                g["flags_ok"] += 1
            if d > TOL_PT or not set(m["flags"]) <= set(e["flags"]):
                misses.append((it["id"], m["what"], m["flags"], m["box"],
                               f"closest {e['what']} {[e['x0'], e['y0'], e['x1'], e['y1']]} off {d:.2f} pt, flags {e['flags']}"))
        for ei, e in enumerate(entries):
            if ei not in used:
                extra[e["what"]] += 1

    print(f"split {split}, {len(items)} pages, tolerance {TOL_PT} pt")
    print(f"{'kind':<8} {'expected flags':<26} {'marks':>5} {'found':>11} {'flags ok':>9}  worst found  median err")
    tot = [0, 0, 0]
    for (what, flags), g in sorted(groups.items()):
        e = sorted(g["errs"])
        med = e[len(e) // 2] if e else float("nan")
        print(f"{what:<8} {flags or '-':<26} {g['marks']:>5} {g['found']:>5} {100 * g['found'] / g['marks']:5.1f}% {g['flags_ok']:>9}"
              f"  {g['worst']:8.3f} pt  {med:8.3f} pt")
        tot[0] += g["marks"]; tot[1] += g["found"]; tot[2] += g["flags_ok"]
    print(f"{'all':<35} {tot[0]:>5} {tot[1]:>5} {100 * tot[1] / tot[0]:5.1f}% {tot[2]:>9}")
    print("map regions matching no mark: " + (", ".join(f"{k} {v}" for k, v in sorted(extra.items())) or "none"))
    if show:
        for m in misses:
            print(*m)


if __name__ == "__main__":
    main()
