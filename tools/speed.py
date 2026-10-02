"""Speed of the page map, native: regions-cli over every file in the given folders, several passes,
timing extract() on bytes already in memory (regions-cli's "micros"), each file's median over the
passes. Prints the time a page (a file's time over its pages) as median, 90th percentile and worst,
the time a file, and the total. Keeps only the numbers, never the JSON. Run on a quiet machine:
timings are only comparable within one session (results/exact-tune.md, section 7).

usage: python tools/speed.py [--passes=3] <folder of PDFs>...
"""
import json
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

from rich.console import Console
from rich.progress import MofNCompleteColumn, Progress, TimeElapsedColumn

CLI = Path(__file__).resolve().parent.parent / "regions" / "target" / "release" / "regions-cli.exe"


def one_pass(files, bar, task):
    out = {}
    for i in range(0, len(files), 40):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
            f.write("\n".join(p.as_posix() for p in files[i:i + 40]))
            lst = f.name
        proc = subprocess.Popen([str(CLI), "--list", lst], stdout=subprocess.PIPE, text=True, encoding="utf-8")
        for line in proc.stdout:
            d = json.loads(line)
            if "micros" in d:
                out[Path(d["file"]).name] = (d["micros"], max(len(d.get("pages", [])), 1))
            bar.advance(task)
        proc.wait()
        Path(lst).unlink()
    return out


def pct(v, q):
    s = sorted(v)
    return s[min(len(s) - 1, int(q * len(s)))]


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opt = dict(a[2:].split("=", 1) for a in sys.argv[1:] if a.startswith("--") and "=" in a)
    passes = int(opt.get("passes", 3))
    if not args:
        sys.exit(__doc__)
    for folder in args:
        files = sorted(Path(folder).glob("*.pdf"))
        runs = []
        # the bar goes to stderr, so the results on stdout stay clean
        with Progress(*Progress.get_default_columns(), TimeElapsedColumn(), MofNCompleteColumn(), transient=True,
                      console=Console(stderr=True)) as bar:
            for k in range(passes):
                task = bar.add_task(f"{Path(folder).name} pass {k + 1}/{passes}", total=len(files))
                runs.append(one_pass(files, bar, task))
        names = [n for n in runs[0] if all(n in r for r in runs)]
        file_ms = {n: statistics.median(r[n][0] for r in runs) / 1000 for n in names}
        pages = {n: runs[0][n][1] for n in names}
        per_page = [file_ms[n] / pages[n] for n in names]
        worst = max(names, key=lambda n: file_ms[n] / pages[n])
        print(f"{Path(folder).name}: {len(names)} files, {sum(pages.values())} pages, {passes} passes")
        print(f"  a page: median {statistics.median(per_page):.2f} ms, 90th percentile {pct(per_page, 0.9):.2f} ms, "
              f"worst {file_ms[worst] / pages[worst]:.1f} ms ({worst}, {pages[worst]} pages)")
        print(f"  a file: median {statistics.median(file_ms.values()):.2f} ms, 90th percentile {pct(list(file_ms.values()), 0.9):.1f} ms")
        print(f"  all files: {sum(file_ms.values()) / 1000:.2f} s, {sum(file_ms.values()) / sum(pages.values()):.2f} ms a page")


if __name__ == "__main__":
    main()
