"""Score regions-cli on the constructed set, per kind.

Runs the binary on every case of one split (tune by default) and checks each against the manifest:

  ocr boxes     found when one "ocr" region covers at least COVER of the box
  text_layer    right when a "text_layer" region covers the box and no "ocr" region does
  box edges     worst edge error of each found ocr box, against the 1 pt target
  none boxes    flagged when any kept region covers COVER of the box (option A is expected to flag
                photos, logos and blanks; the numbers are reported, not hidden)
  rule, background   right when no kept region covers the box
  control       right when the page has no kept region at all
  full_scan     right when there's exactly one kept region and it's an "ocr" one covering the page

usage: python tools/score_regions.py [split=tune] [--misses]
"""
import json
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT / "regions" / "target" / "release" / "regions-cli.exe"
COVER = 0.90


def covered(box, region):
    """Share of box inside region."""
    x0, y0, x1, y1 = box
    dx = min(x1, region["x1"]) - max(x0, region["x0"])
    dy = min(y1, region["y1"]) - max(y0, region["y0"])
    area = (x1 - x0) * (y1 - y0)
    return max(dx, 0) * max(dy, 0) / area if area > 0 else 0.0


def run(files):
    """One JSON line per file, from the binary; Windows paths so the .exe reads them."""
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
        f.write("\n".join(str(p.resolve()).replace("\\", "/") for p in files))
        lst = f.name
    out = subprocess.run([str(CLI), "--list", lst], capture_output=True, text=True, encoding="utf-8", check=True).stdout
    Path(lst).unlink()
    return {Path(d["file"]).name: d for d in map(json.loads, out.splitlines())}


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    split = args[0] if args else "tune"
    show = "--misses" in sys.argv
    if split != "tune":
        sys.exit("only the tune split is scored until the rules are frozen")
    man = json.loads((ROOT / "data" / "constructed" / "manifest.json").read_text(encoding="utf-8"))
    cases = [c for c in man["items"] if c["split"] == split]
    res = run([ROOT / "data" / "constructed" / c["file"] for c in cases])

    tally = defaultdict(lambda: [0, 0])  # kind -> [right, total]
    misses = []
    edges = []  # worst edge error, points, of each found ocr box
    for c in cases:
        d = res[Path(c["file"]).name]
        page = d["pages"][0]
        kept = page["regions"]
        ocr = [r for r in kept if r["kind"] == "ocr"]
        k = c["kind"]
        if k == "control":
            ok = not kept
        elif k == "full_scan":
            box = [0, 0, page["width"], page["height"]]
            ok = len(kept) == 1 and bool(ocr) and covered(box, ocr[0]) >= COVER
        else:
            ok = True
            for g in c["regions"]:
                box = g["box"]
                if g["expect"] == "ocr":
                    hit = [r for r in ocr if covered(box, r) >= COVER]
                    ok &= bool(hit)
                    if hit:
                        edges.append(max(abs(a - b) for a, b in zip(box, [hit[0][e] for e in ("x0", "y0", "x1", "y1")])))
                elif g["expect"] == "text_layer":
                    ok &= any(r["kind"] == "text_layer" and covered(box, r) >= COVER for r in kept) \
                        and not any(covered(box, r) >= COVER for r in ocr)
                else:
                    # right = not flagged; for photo/logo/blank "right" is what option B would aim at
                    ok &= not any(covered(box, r) >= COVER for r in kept)
        tally[k][0] += ok
        tally[k][1] += 1
        if not ok:
            misses.append((c["id"], k, c.get("wrap"), c.get("placement"), [(r["kind"], r["x0"], r["y0"], r["x1"], r["y1"]) for r in kept],
                           [(r["kind"], r["x0"], r["y0"], r["x1"], r["y1"]) for r in page["dropped"]]))

    what = {
        "text": "ocr box found", "text_ocr": "text_layer, not ocr", "full_scan": "one full-page ocr region",
        "control": "zero regions", "rule": "never a region", "background": "never a region",
        "photo": "not flagged", "logo": "not flagged", "blank": "not flagged",
    }
    print(f"split {split}, {len(cases)} cases")
    print(f"{'kind':<11} {'check':<26} {'right':>9}")
    for k in ["text", "text_ocr", "full_scan", "control", "rule", "background", "photo", "logo", "blank"]:
        if k in tally:
            r, n = tally[k]
            print(f"{k:<11} {what[k]:<26} {r:>4}/{n:<4} {100 * r / n:5.1f}%")
    if edges:
        edges.sort()
        print(f"ocr box edges within 1 pt: {sum(e <= 1 for e in edges)}/{len(edges)}, worst {edges[-1]:.2f} pt")
    for k in ["photo", "logo", "blank"]:
        if k in tally:
            r, n = tally[k]
            print(f"{k} flagged as ocr (option A can't tell): {n - r}/{n}")
    if show:
        for m in misses:
            print(*m)


if __name__ == "__main__":
    main()
