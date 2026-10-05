"""Score the kind layer for images (regions::kind, plans/chunk8-kind-images.md 8e) on tuning data.

Runs `thumbs - --kinds` and compares each image's kind with the truth:

  constructed   each case's pasted image has a known kind: text, text_ocr and full_scan are text
                images; photo is a photo; logo a graphic; blank, background and rule have no text.
                Targets (D84): text images found at least 95%; photos, logos, blanks, backgrounds
                and rules not called text at least 95%.
  govdocs1 003  each labelled placement is text, none or unsure by Tesseract (D66, D67), matched by
                object number. A text kind is right on a text label; any other kind is right on none.
                Unsure placements are reported apart.

Has-text (D86, D87): a second confidence that the image holds text, whatever its kind. On the
constructed set text images should have it and photos, blanks, backgrounds and rules shouldn't
(logos are reported, not scored: some hold letters). On 003 it's scored against labels from the
image's own pixels (data/real/tune-003-image.jsonl, tools/label_images.py), unsure counting as
text (D67); "held" means has_text at or above HAS_CUT.

Calibration (D84): by confidence band, the share right. Target: at 0.9 or more, at least 90% right.
Inline images (no object) and images that don't decode are counted apart, never dropped.

usage: python tools/score_kinds.py [constructed] [real] [sodir] [--heldout] [--misses]
--heldout scores the constructed heldout split and govdocs1 004 (region labels heldout-004.jsonl,
image labels heldout-004-image.jsonl) and the Sodir held-out pages, and is refused unless
tools/check_frozen.py passes (8f).

sodir scores has-text per scanned Sodir page (tools/label_sodir.py, data/real/sodir-labels.jsonl).
"""
import json
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
THUMBS = ROOT / "regions" / "target" / "release" / "thumbs.exe"
GOVDOCS = Path("C:/Users/sharad/Downloads/05_Research-Reference/Datasets/govdocs1")
REAL = GOVDOCS / "003"
TEXT = {"text", "text_ocr", "full_scan"}
BANDS = [(0.9, 1.01), (0.7, 0.9), (0.5, 0.7), (0.0, 0.5)]
HAS_CUT = 0.5


def kinds(files):
    """One entry per (file name, obj) from thumbs --kinds, in batches so no call runs long."""
    out = {}
    for i in range(0, len(files), 40):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
            f.write("\n".join(Path(p).resolve().as_posix() for p in files[i:i + 40]))
            lst = f.name
        res = subprocess.run([str(THUMBS), "-", "--kinds", "--list", lst], capture_output=True, text=True, encoding="utf-8").stdout
        Path(lst).unlink()
        for line in res.splitlines():
            j = json.loads(line)
            out[(Path(j["file"]).name, j["obj"])] = j
    return out


def band_of(c):
    return next(b for b in BANDS if b[0] <= c < b[1])


def has_band(h):
    """The band of has-text's own confidence in its answer (h for yes, 1 - h for no)."""
    return band_of(max(h, 1 - h))


def constructed(calib, misses, hcalib, split="tune"):
    man = json.loads((ROOT / "data" / "constructed" / "manifest.json").read_text(encoding="utf-8"))
    items = [c for c in man["items"] if c["split"] == split]
    got = kinds([ROOT / "data" / "constructed" / c["file"] for c in items])
    by_file = defaultdict(list)
    for (name, _), j in got.items():
        by_file[name].append(j)
    table = defaultdict(Counter)
    found = Counter()
    has = Counter()
    apart = Counter()
    miss = []
    for c in items:
        if c["kind"] == "control":
            continue
        js = by_file.get(Path(c["file"]).name, [])
        if not js:
            apart[f"{c['kind']}: no image object (inline)" if c.get("wrap") == "inline" else f"{c['kind']}: no image object"] += 1
            continue
        ok = [j for j in js if "kind" in j]
        if not ok:
            apart[f"{c['kind']}: {js[0].get('error')}"] += 1
            continue
        # the pasted image is the largest one the page draws (strips are separate objects of one image)
        j = max(ok, key=lambda j: j["width"] * j["height"])
        k, conf = j["kind"], j["confidence"]
        table[c["kind"]][k] += 1
        want_text = c["kind"] in TEXT
        right = (k == "text") == want_text
        found[(want_text, right)] += 1
        calib[band_of(conf)][right] += 1
        if not right:
            miss.append((c["id"], c["kind"], k, conf))
        if c["kind"] != "logo":
            held = j["has_text"] >= HAS_CUT
            hright = held == want_text
            has[(want_text, hright)] += 1
            hcalib[has_band(j["has_text"])][hright] += 1
            if not hright:
                miss.append((c["id"], c["kind"], "has_text", j["has_text"]))
        else:
            has["logo held"] += j["has_text"] >= HAS_CUT
            has["logo"] += 1
    print(f"constructed {split}: kind of the pasted image, by case kind")
    for kind in sorted(table):
        print(f"  {kind:11s} {dict(table[kind].most_common())}")
    t_ok, t_all = found[(True, True)], found[(True, True)] + found[(True, False)]
    n_ok, n_all = found[(False, True)], found[(False, True)] + found[(False, False)]
    print(f"  text images found: {t_ok}/{t_all} ({100 * t_ok / max(t_all, 1):.1f}%, target 95%)")
    print(f"  other images not called text: {n_ok}/{n_all} ({100 * n_ok / max(n_all, 1):.1f}%, target 95%)")
    ht, hn = has[(True, True)], has[(False, True)]
    print(f"  has-text on text images: {ht}/{ht + has[(True, False)]}; not on photos, blanks, backgrounds, rules: {hn}/{hn + has[(False, False)]}; logos held: {has['logo held']}/{has['logo']}")
    print(f"  counted apart: {dict(apart)}")
    if misses:
        for m in miss:
            print("   miss", m)


def real_has_text(got, hcalib, misses, path, name):
    if not path.exists():
        print(f"govdocs1 {name} has-text: no image labels yet (tools/label_images.py)")
        return
    labs = [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]
    # a merged image (strips) is judged whole, so its truth is whole too: it holds text when any of
    # its strips does; each image counts once (8g)
    groups = {}
    apart = Counter()
    for lab in labs:
        if "label" not in lab:
            apart["label error"] += 1
            continue
        j = got.get((lab["file"], lab["xref"]))
        if j is None or "kind" not in j:
            apart["not placed" if j is None else j.get("error")] += 1
            continue
        key = (lab["file"], j.get("image", lab["xref"]))
        g = groups.setdefault(key, {"file": lab["file"], "xref": key[1], "label": "none", "sample": [], "j": j, "strips": 0})
        g["strips"] += 1
        if lab["label"] in ("text", "unsure"):
            g["label"] = "text"
            g["sample"] = g["sample"] or lab.get("sample", [])
    tab = Counter()
    miss = []
    labs = list(groups.values())
    for lab in labs:
        j = lab["j"]
        want = lab["label"] == "text"
        held = j["has_text"] >= HAS_CUT
        tab[(want, held)] += 1
        hcalib[has_band(j["has_text"])][held == want] += 1
        if held != want:
            miss.append((lab["file"], lab["xref"], lab["label"], j["kind"], j["has_text"], j["features"]["word_marks"], lab.get("sample", [])[:4]))
    tp, fn, fp, tn = tab[(True, True)], tab[(True, False)], tab[(False, True)], tab[(False, False)]
    print(f"govdocs1 {name} has-text against image-only labels ({len(labs)} images, merged strips as one, unsure as text):")
    print(f"  text found {tp}/{tp + fn} ({100 * tp / max(tp + fn, 1):.1f}%); none not held {tn}/{tn + fp} ({100 * tn / max(tn + fp, 1):.1f}%); counted apart {dict(apart)}")
    if misses:
        for m in miss[:60]:
            print("   has-text miss", m)


def real(calib, misses, hcalib=None, name="003", labels="tune-003.jsonl", image_labels="tune-003-image.jsonl"):
    rows = [json.loads(l) for l in (ROOT / "data" / "real" / labels).read_text(encoding="utf-8").splitlines() if l.strip()]
    files = sorted({r["file"] for r in rows})
    got = kinds([GOVDOCS / name / f for f in files])
    if hcalib is not None:
        real_has_text(got, hcalib, misses, ROOT / "data" / "real" / image_labels, name)
    table = defaultdict(Counter)
    apart = Counter()
    miss = []
    for r in rows:
        for g in r["regions"]:
            lab = g["label"]
            j = got.get((r["file"], g.get("xref")))
            if not g.get("xref"):
                apart["inline"] += 1
                continue
            if j is None:
                apart["not placed by the map"] += 1
                continue
            if "kind" not in j:
                apart[j.get("error")] += 1
                continue
            k, conf = j["kind"], j["confidence"]
            table[lab][k] += 1
            if lab == "unsure":
                continue
            right = (k == "text") == (lab == "text")
            calib[band_of(conf)][right] += 1
            if not right:
                miss.append((r["file"], r["page"], g["xref"], lab, k, conf, g.get("sample", [])[:4]))
    print(f"govdocs1 {name}: kind by Tesseract region label (placements)")
    for lab in ("text", "none", "unsure"):
        print(f"  {lab:7s} {dict(table[lab].most_common())}")
    tt = table["text"]
    nn = table["none"]
    print(f"  text labels called text: {tt['text']}/{sum(tt.values())}; none labels not called text: {sum(nn.values()) - nn['text']}/{sum(nn.values())}")
    print(f"  counted apart: {dict(apart)}")
    if misses:
        for m in miss[:60]:
            print("   miss", m)


SODIR = Path("data/sodir")


def sodir(hcalib, misses, split="tune"):
    """Has-text per Sodir scanned page against its Tesseract label (tools/label_sodir.py): the page holds
    text when any image on it is held (has-text at HAS_CUT or more). Unsure counts as text (D67)."""
    lab_path = ROOT / "data" / "real" / "sodir-labels.jsonl"
    if not lab_path.exists():
        print("sodir: no labels yet (tools/label_sodir.py)")
        return
    strata = {(j["file"], j["page"]): j["stratum"] for j in map(json.loads, filter(str.strip, (ROOT / "data" / "real" / "sodir-pages.jsonl").read_text(encoding="utf-8").splitlines()))}
    labels = [j for j in map(json.loads, filter(str.strip, lab_path.read_text(encoding="utf-8").splitlines())) if j["split"] == split and "label" in j]
    files = sorted({j["file"] for j in labels})
    best = {}
    for i in range(0, len(files), 10):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
            f.write("\n".join((SODIR / x).as_posix() for x in files[i:i + 10]))
            lst = f.name
        res = subprocess.run([str(THUMBS), "-", "--kinds", "--list", lst], capture_output=True, text=True, encoding="utf-8").stdout
        Path(lst).unlink()
        for line in res.splitlines():
            j = json.loads(line)
            if "has_text" not in j:
                continue
            key = (Path(j["file"]).relative_to(SODIR).as_posix(), j["page"])
            if key not in best or j["has_text"] > best[key]["has_text"]:
                best[key] = j
    found, apart, miss = defaultdict(Counter), 0, []
    for lab in labels:
        key = (lab["file"], lab["page"])
        k = best.get(key)
        if k is None:
            apart += 1
            continue
        truth = lab["label"] != "none"
        held = k["has_text"] >= HAS_CUT
        found[(strata.get(key, "?"), truth)][held] += 1
        hcalib[has_band(k["has_text"])][held == truth] += 1
        if held != truth:
            miss.append(f"{lab['file']} p{lab['page']} {lab['label']} ({lab['ocr_words']} words) {k['w']}x{k['h']} kind {k['kind']} has_text {k['has_text']} marks {k['features']['word_marks']} glyphs {k['features']['glyphs']} aligned {k['features']['aligned']:.2f}")
    t = sum((found[(s_, True)] for s_ in ("low", "high")), Counter())
    nn = sum((found[(s_, False)] for s_ in ("low", "high")), Counter())
    print(f"sodir {split} has-text by page ({len(labels)} labelled pages, unsure as text):")
    print(f"  text found {t[True]}/{t[True] + t[False]}; none not held {nn[False]}/{nn[True] + nn[False]}; no decoded image {apart}")
    for s_ in ("low", "high"):
        a, b = found[(s_, True)], found[(s_, False)]
        print(f"  stratum {s_}: text found {a[True]}/{a[True] + a[False]}, none not held {b[False]}/{b[True] + b[False]}")
    if misses:
        for m in miss[:80]:
            print("   miss", m)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    misses = "--misses" in sys.argv
    held = "--heldout" in sys.argv or any(a in ("heldout", "004") for a in args)
    if held:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        from check_frozen import require_frozen
        require_frozen()
    calib = defaultdict(Counter)
    hcalib = defaultdict(Counter)
    if not args or "constructed" in args:
        constructed(calib, misses, hcalib, "heldout" if held else "tune")
    if not args or "real" in args:
        if held:
            real(calib, misses, hcalib, "004", "heldout-004.jsonl", "heldout-004-image.jsonl")
        else:
            real(calib, misses, hcalib)
    if not args or "sodir" in args:
        sodir(hcalib, misses, "heldout" if held else "tune")
    for name, cal in (("kind", calib), ("has-text", hcalib)):
        print(f"calibration of {name} (share right by confidence band; target: 0.9 and up at least 90%):")
        for b in BANDS:
            c = cal[b]
            n = c[True] + c[False]
            print(f"  {b[0]:.1f} to {min(b[1], 1.0):.1f}: {c[True]}/{n}" + (f" ({100 * c[True] / n:.1f}%)" if n else ""))


if __name__ == "__main__":
    main()
