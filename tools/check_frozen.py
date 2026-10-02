"""Check the frozen source: every file in results/frozen.sha256 must hash as recorded, and the
release binary must be newer than all of them.

What is frozen (plans/chunk7-freeze.md, D76; the kind layer joined in 8f, the vector kinds in 9d):
regions/Cargo.toml, Cargo.lock, every file in regions/src except ocr.rs (the map, pixels, the
decoders, kind.rs, vkind.rs), the thumbs and vkinds tools, the tools that build the test sets, label
the images and measure the map and the kinds, the three set manifests (each lists a sha256 per PDF),
and this file. ocr.rs and
score_regions.py are left out: the OCR scoring belongs to the next repo.

Line endings are normalised to LF first, so a checkout that converts them doesn't count as a change.
The held-out guards in ink_check.py, score_map.py and score_regions.py call require_frozen().

usage: python tools/check_frozen.py            check
       python tools/check_frozen.py --write    record the current source (at the freeze only)
"""
import hashlib
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RECORD = ROOT / "results" / "frozen.sha256"
CLI = ROOT / "regions" / "target" / "release" / "regions-cli.exe"
THUMBS = ROOT / "regions" / "target" / "release" / "thumbs.exe"
VKINDS = ROOT / "regions" / "target" / "release" / "vkinds.exe"
TOOLS = ["tools/check_frozen.py", "tools/diff_exact.py", "tools/ink_check.py", "tools/score_map.py", "tools/build_marks.py", "tools/build_set.py",
         "tools/score_kinds.py", "tools/label_images.py", "tools/select_real.py", "regions/src/bin/thumbs.rs",
         "tools/build_vectors.py", "tools/score_vkinds.py", "regions/src/bin/vkinds.rs"]
MANIFESTS = ["data/constructed/manifest.json", "data/constructed/marks/manifest.json", "data/constructed/vectors/manifest.json"]


def names():
    src = sorted(p.relative_to(ROOT).as_posix() for p in (ROOT / "regions" / "src").glob("*.rs") if p.name != "ocr.rs")
    return ["regions/Cargo.toml", "regions/Cargo.lock"] + src + TOOLS + MANIFESTS


def digest_of(name):
    return hashlib.sha256((ROOT / name).read_bytes().replace(b"\r\n", b"\n")).hexdigest()


def problems():
    """What stops a held-out run: changed or missing files, a new source file, a stale binary."""
    if not RECORD.exists():
        return ["no results/frozen.sha256: the rules aren't frozen"]
    out = []
    recorded = {}
    for line in RECORD.read_text(encoding="utf-8").splitlines():
        digest, name = line.split(None, 1)
        recorded[name] = digest
        if not (ROOT / name).exists():
            out.append(f"MISSING {name}")
        elif digest_of(name) != digest:
            out.append(f"CHANGED {name}")
    out += [f"NEW {n} (not in the record)" for n in names() if n not in recorded]
    # each binary against the sources built into it: regions-cli doesn't contain src/bin/thumbs.rs
    for exe, skip in ((CLI, "regions/src/bin/"), (THUMBS, None), (VKINDS, None)):
        if not exe.exists():
            out.append(f"no release binary {exe.name}: cargo build --release")
        elif any((ROOT / n).stat().st_mtime > exe.stat().st_mtime for n in recorded
                 if n.startswith("regions/") and not (skip and n.startswith(skip)) and (ROOT / n).exists()):
            out.append(f"{exe.name} is older than the source: cargo build --release")
    return out


def require_frozen():
    bad = problems()
    if bad:
        sys.exit("held-out data needs the frozen source:\n  " + "\n  ".join(bad))


if __name__ == "__main__":
    if "--write" in sys.argv:
        ns = names()
        RECORD.write_text("".join(f"{digest_of(n)}  {n}\n" for n in ns), encoding="utf-8", newline="\n")
        print(f"recorded {len(ns)} files")
        sys.exit(0)
    bad = problems()
    print("frozen source: " + ("ok" if not bad else f"{len(bad)} problem(s)"))
    for b in bad:
        print("  " + b)
    sys.exit(1 if bad else 0)
