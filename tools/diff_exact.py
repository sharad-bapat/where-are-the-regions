"""Prove the exact layer's output didn't change: run an older regions-cli and the current one over
the same files and compare every JSON line, ignoring the timing ("micros") and, with --allow, any
top-level page keys a later chunk adds. Run before each relock (plans/chunk8-kind-images.md).

usage: python tools/diff_exact.py <old regions-cli.exe> [--allow=key,key] <folder or list.txt>...
Folders are searched for *.pdf (not recursively). No rendering; one file at a time.
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
NEW = ROOT / "regions" / "target" / "release" / "regions-cli.exe"


def files_of(arg):
    p = Path(arg)
    if p.is_dir():
        return sorted(p.glob("*.pdf"))
    return [Path(l.strip()) for l in p.read_text(encoding="utf-8").splitlines() if l.strip()]


def out(exe, f):
    r = subprocess.run([str(exe), f.as_posix()], capture_output=True, text=True, encoding="utf-8")
    d = json.loads(r.stdout)
    d.pop("micros", None)
    return d


def strip(d, allow):
    for p in d.get("pages", []):
        for k in allow:
            p.pop(k, None)
    return d


def main():
    args = sys.argv[1:]
    if len(args) < 2:
        sys.exit(__doc__)
    old = Path(args.pop(0))
    allow = []
    for a in list(args):
        if a.startswith("--allow="):
            allow = [k for k in a.split("=", 1)[1].split(",") if k]
            args.remove(a)
    files = [f for a in args for f in files_of(a)]
    diff = 0
    for f in files:
        a, b = strip(out(old, f), allow), strip(out(NEW, f), allow)
        if a != b:
            diff += 1
            if diff <= 10:
                print(f"DIFFERS {f}")
    print(f"{len(files)} files, {diff} differ")
    sys.exit(1 if diff else 0)


if __name__ == "__main__":
    main()
