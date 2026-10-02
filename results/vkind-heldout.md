# Kind layer for vectors, held-out results (2 October 2026)

The vector kinds (regions/src/vkind.rs: rule, border, table grid, fill, chart or diagram, outlined text) measured once on data they were never tuned on. The rules were frozen first: tools/check_frozen.py now records the vector set builder, the scorer, the vkinds tool and the vector set's manifest with the rest (34 files), and it passed before the run. The targets were fixed before any held-out data was opened (D90). Output: results/vkinds-heldout.txt.

## Summary

| Target (D90) | Result | |
| --- | --- | --- |
| Constructed heldout: at least 90% of clusters given their right kind | 117 of 117 (100%) | met |
| Constructed heldout: table grids found, at least 90% | 21 of 21 | met |
| Constructed heldout: outlined text found, at least 90% | 19 of 19 | met |
| Kind at confidence 0.9 or more, at least 90% right (constructed and 004 labels together, as on tune) | 136 of 168 (81.0%) | missed |

The constructed set is drawn by tools/build_vectors.py from the same recipes as the tune split, so meeting its targets says the rules do what they were written for. The real pages are the test that matters, and there the layer falls short.

## govdocs1 004 hand labels

150 clusters sampled as for 003: from the 704 labelled pages of thread 004, 25 per predicted kind, seed 20260930, shuffled together onto 13 contact sheets that showed no prediction, and labelled by eye (by Claude) before the scoring run. The predicted kind of each item was kept in a separate file until every label was written. Labels are in data/real/heldout-004-vectors.jsonl; 10 were too small or faint to judge and are counted apart.

| True kind | Right | Given instead |
| --- | --- | --- |
| rule | 22 of 27 | chart or diagram 5 |
| outlined text | 22 of 36 | chart or diagram 8, fill 4, rule 1, border 1 |
| border | 12 of 22 | table grid 4, fill 4, chart or diagram 2 |
| table grid | 8 of 10 | chart or diagram 1, border 1 |
| chart or diagram | 8 of 35 | table grid 12, fill 8, border 7 |
| fill | 3 of 10 | border 3, rule 2, chart or diagram 1, table grid 1 |
| all | 75 of 140 (53.6%) | |

On the 003 tune labels the same count was 104 of 147 (70.7%).

Calibration on the 004 labels alone:

| Band | 004 (held out) | 003 (tune) |
| --- | --- | --- |
| 0.9 and up | 64 of 96 (66.7%) | 92 of 102 (90.2%) |
| 0.7 to 0.9 | 11 of 36 (30.6%) | 4 of 27 (14.8%) |
| 0.5 to 0.7 | 0 of 8 | 8 of 18 (44.4%) |

So the 90% at 0.9 that tune reached on real pages doesn't hold on new files: the rules fit 003's 46 files. The most common confident misses:

- 12 charts given "table grid" at 0.9. On the contact sheets 10 of them are framed plots, an axis box with tick marks (3 also with grid lines), and 2 are diagrams with callout lines.
- 7 charts given "border" at 0.9, and 8 given "fill" at 0.85.
- 7 outlined words given "chart or diagram" at 0.7, and 4 given "fill" at 0.85.

Only the first group was looked at on the sheets; the causes of the others haven't been checked one by one.

Some of the disagreement is in the labels' definitions rather than the rules: a whole slide that one cluster covers, or a framed figure with a plot inside, could fairly be called a border or a chart. The labels weren't revisited after the run.

## What it means

The exact layer (where every mark is) is reliable; the vector kinds are not, beyond rules and outlined text. Until they are reworked against more varied tuning data, a write-up should present the vector kinds as experimental, without the calibration claim, and the OCR routing should not lean on them. Reworking them would be a new chunk on tuning data only, with a fresh held-out sample afterwards (the 004 labels can't be used again as a held-out test once rules are changed to fit them).

## Reproduce

```
python tools/check_frozen.py
python tools/score_vkinds.py --heldout --misses
```
