"""Label the Sodir scanned pages (data/real/sodir-pages.jsonl) with Tesseract, for has-text on sparse
and dense scans (results/kinds-turn.md, "Still open").

The pages are scanned well reports from the Norwegian Offshore Directorate's FactPages, picked from
pages with no text layer: 160 for tuning and 160 held out, split by file, so no file is in both. Each
line of the page list gives the file's URL and sha256, so the set can be downloaded again; the PDFs
themselves aren't redistributed.

Each page is rendered as displayed at DPI and read with the same rules as the other labels (D67): a
word counts at confidence MIN_CONF or more with 3 letters or 2 digits; TEXT_WORDS or more words is
text, none is none, in between is unsure (scored as text). One page is one label, since these pages
are a single scan each. Resumable: pages already in the output are skipped.

usage: python tools/label_sodir.py [sodir snapshot dir] [out.jsonl]
"""
import hashlib
import json
import sys
from pathlib import Path

import fitz
from rich.progress import BarColumn, MofNCompleteColumn, Progress, TextColumn, TimeElapsedColumn, TimeRemainingColumn

sys.path.insert(0, str(Path(__file__).parent))
from select_real import DPI, MIN_CONF, TEXT_WORDS, ocr_words  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SODIR = Path("data/sodir")
PAGES = ROOT / "data" / "real" / "sodir-pages.jsonl"


def main():
    src = Path(sys.argv[1]) if len(sys.argv) > 1 else SODIR
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else ROOT / "data" / "real" / "sodir-labels.jsonl"
    pages = [json.loads(l) for l in PAGES.read_text(encoding="utf-8").splitlines() if l.strip()]
    done = set()
    if out.exists():
        done = {(j["file"], j["page"]) for j in map(json.loads, filter(str.strip, out.read_text(encoding="utf-8").splitlines()))}
    todo = [p for p in pages if (p["file"], p["page"]) not in done]
    checked = {}
    counts = {"text": 0, "unsure": 0, "none": 0, "error": 0}
    cols = (TextColumn("{task.description}"), BarColumn(), MofNCompleteColumn(), TimeElapsedColumn(), TimeRemainingColumn(),
            TextColumn("text {task.fields[text]}  unsure {task.fields[unsure]}  none {task.fields[none]}  errors {task.fields[error]}"))
    print(f"{len(done)} labelled already, {len(todo)} to go, rules: {DPI} dpi, confidence {MIN_CONF}, {TEXT_WORDS} words for text")
    with Progress(*cols) as bar, open(out, "a", encoding="utf-8") as f:
        task = bar.add_task("labelling", total=len(todo), **counts)
        for p in todo:
            path = src / p["file"]
            try:
                if p["file"] not in checked:
                    checked[p["file"]] = hashlib.sha256(path.read_bytes()).hexdigest() == p["sha256"]
                if not checked[p["file"]]:
                    raise ValueError("sha256 differs from the page list")
                with fitz.open(path) as doc:
                    words = ocr_words(doc[p["page"] - 1].get_pixmap(dpi=DPI))
                n = len(words)
                label = "text" if n >= TEXT_WORDS else "none" if n == 0 else "unsure"
                rec = {"file": p["file"], "page": p["page"], "split": p["split"], "ocr_words": n, "label": label, "sample": words[:8]}
            except Exception as e:
                label = "error"
                rec = {"file": p["file"], "page": p["page"], "split": p["split"], "error": f"{type(e).__name__}: {e}"[:120]}
            f.write(json.dumps(rec) + "\n")
            f.flush()
            counts[label] += 1
            bar.update(task, advance=1, **counts)
    print(f"written: {out}")


if __name__ == "__main__":
    main()
