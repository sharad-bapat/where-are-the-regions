"""Image placements from regions-cli against the constructed set's known boxes.

Every constructed case draws exactly one pasted image (merged, when it was split into strips) at its
manifest box, except controls (none). The source pages have no images of their own. A case passes
when the tool reports one image and its edges are within TOL points of the box.

usage: python tools/check_images.py <constructed dir> [split=tune]
"""
import collections
import json
import subprocess
import sys
from pathlib import Path

CLI = Path(__file__).resolve().parent.parent / "regions" / "target" / "release" / "regions-cli"
TOL = 1.0


def main():
    root = Path(sys.argv[1])
    split = sys.argv[2] if len(sys.argv) > 2 else "tune"
    items = [c for c in json.loads((root / "manifest.json").read_text(encoding="utf-8"))["items"] if c["split"] == split]
    lst = root / f"{split}-files.txt"
    lst.write_text("\n".join(str(root / c["file"]) for c in items), encoding="utf-8")
    out = subprocess.run([str(CLI), "--list", str(lst)], capture_output=True, text=True, encoding="utf-8").stdout
    got = {Path(r["file"]).name: r for r in map(json.loads, out.splitlines())}
    tally = collections.Counter()
    fails = []
    for c in items:
        r = got[Path(c["file"]).name]
        imgs = r["pages"][0]["images"] if r.get("pages") else []
        key = (c["kind"], c.get("wrap"))
        if c["kind"] == "control":
            ok = not imgs
            why = f"{len(imgs)} images"
        else:
            box = c["regions"][0]["box"]
            ok = len(imgs) == 1 and all(abs(a - b) <= TOL for a, b in zip([imgs[0][k] for k in ("x0", "y0", "x1", "y1")], box))
            why = f"{len(imgs)} images {[[i[k] for k in ('x0','y0','x1','y1')] + [i['parts']] for i in imgs][:3]} vs {[round(v, 1) for v in box]}"
        tally[key, ok] += 1
        if not ok:
            fails.append((c["id"], key, why))
    for k in sorted({k for k, _ in tally}, key=str):
        print(f"{str(k):32} {tally[k, True]:4} / {tally[k, True] + tally[k, False]}")
    for f in fails[:20]:
        print("FAIL", *f)
    n_ok = sum(v for (k, ok), v in tally.items() if ok)
    print(f"{n_ok} of {len(items)} cases placed within {TOL} pt")


if __name__ == "__main__":
    main()
