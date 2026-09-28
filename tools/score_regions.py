"""Score regions-cli on the constructed set, per kind.

Every image is a region with a confidence (D69); a region is "flagged" when its confidence is at or
above the cutoff (0.4 by default, regions::ocr::CUT). Runs the binary on every case of one split
(tune by default) and checks each against the manifest:

  ocr boxes     found when one flagged region covers at least COVER of the box
  text_layer    right when a region with the text_layer reason covers the box and no flagged one does
  box edges     worst edge error of each found ocr box, against the 1 pt target
  none boxes    flagged when any flagged region covers COVER of the box (option A is expected to flag
                photos, logos and blanks; the numbers are reported, not hidden)
  rule, background   right when no flagged region covers the box
  control       right when the page has no flagged region at all
  full_scan     right when there's exactly one flagged region and it covers the page

For each kind it also prints the spread of confidence of the region that best covers each box (or
the page's highest for control), so a cutoff can be read off rather than guessed.

usage: python tools/score_regions.py [split=tune] [--cut=0.4] [--misses]
"""
import json
import statistics
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


def reasons(r):
    return [w for w, _ in r["reasons"]]


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    split = args[0] if args else "tune"
    show = "--misses" in sys.argv
    cut = next((float(a.split("=", 1)[1]) for a in sys.argv if a.startswith("--cut=")), 0.4)
    if split != "tune":
        sys.exit("only the tune split is scored until the rules are frozen")
    man = json.loads((ROOT / "data" / "constructed" / "manifest.json").read_text(encoding="utf-8"))
    cases = [c for c in man["items"] if c["split"] == split]
    res = run([ROOT / "data" / "constructed" / c["file"] for c in cases])

    tally = defaultdict(lambda: [0, 0])  # kind -> [right, total]
    conf = defaultdict(list)  # kind -> confidence of the best-covering region per box
    misses = []
    edges = []  # worst edge error, points, of each found ocr box
    for c in cases:
        d = res[Path(c["file"]).name]
        page = d["pages"][0]
        every = page["regions"]
        flagged = [r for r in every if r["confidence"] >= cut]
        k = c["kind"]
        if k == "control":
            ok = not flagged
            conf[k].append(max((r["confidence"] for r in every), default=0.0))
        elif k == "full_scan":
            box = [0, 0, page["width"], page["height"]]
            ok = len(flagged) == 1 and covered(box, flagged[0]) >= COVER
            conf[k].append(max((r["confidence"] for r in every if covered(box, r) >= COVER), default=0.0))
        else:
            ok = True
            for g in c["regions"]:
                box = g["box"]
                over = [r for r in every if covered(box, r) >= COVER]
                conf[k].append(max((r["confidence"] for r in over), default=0.0))
                if g["expect"] == "ocr":
                    hit = [r for r in flagged if covered(box, r) >= COVER]
                    ok &= bool(hit)
                    if hit:
                        edges.append(max(abs(a - b) for a, b in zip(box, [hit[0][e] for e in ("x0", "y0", "x1", "y1")])))
                elif g["expect"] == "text_layer":
                    ok &= any("text_layer" in reasons(r) for r in over) \
                        and not any(covered(box, r) >= COVER for r in flagged)
                else:
                    # right = not flagged; for photo/logo/blank "right" is what option B would aim at
                    ok &= not any(covered(box, r) >= COVER for r in flagged)
        tally[k][0] += ok
        tally[k][1] += 1
        if not ok:
            misses.append((c["id"], k, c.get("wrap"), c.get("placement"),
                           [(r["confidence"], reasons(r), r["x0"], r["y0"], r["x1"], r["y1"]) for r in every]))

    what = {
        "text": "ocr box flagged", "text_ocr": "text_layer, not flagged", "full_scan": "one full-page region",
        "control": "nothing flagged", "rule": "never flagged", "background": "never flagged",
        "photo": "not flagged", "logo": "not flagged", "blank": "not flagged",
    }
    print(f"split {split}, {len(cases)} cases, cut {cut}")
    print(f"{'kind':<11} {'check':<24} {'right':>12}   confidence min / median / max")
    for k in ["text", "text_ocr", "full_scan", "control", "rule", "background", "photo", "logo", "blank"]:
        if k in tally:
            r, n = tally[k]
            v = conf[k]
            spread = f"{min(v):.3f} / {statistics.median(v):.3f} / {max(v):.3f}" if v else ""
            print(f"{k:<11} {what[k]:<24} {r:>4}/{n:<4} {100 * r / n:5.1f}%   {spread}")
    if edges:
        edges.sort()
        print(f"ocr box edges within 1 pt: {sum(e <= 1 for e in edges)}/{len(edges)}, worst {edges[-1]:.2f} pt")
    for k in ["photo", "logo", "blank"]:
        if k in tally:
            r, n = tally[k]
            print(f"{k} flagged (option A can't tell): {n - r}/{n}")
    if show:
        for m in misses:
            print(*m)


if __name__ == "__main__":
    main()
