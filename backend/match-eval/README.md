# Match eval

Measures how well Jev decides whether two people should be matched, on hand-labelled
pairs of summarizer outputs (`match_pairs.json`, same `{topic, task_summary}` shape as
`docs/activity-capture.md`). Use it before changing the Jev question or the threshold.

```sh
cd backend/match-eval
../.venv/bin/python run_eval.py                 # all variants (needs JEV_API)
../.venv/bin/python run_eval.py --variant current
../.venv/bin/python run_eval.py --offline       # word-overlap baseline, no key
```

`JEV_API` is read from the environment, then `backend/match-eval/.env`, then
`backend/fastapi-sultan/.env`. The `current` variant runs Sultan's
`backend/fastapi-sultan/app/jev.py` unchanged, so it measures exactly what the app does.
Pairs with the same `a` go in one request, like production scoring several candidates.

| Variant | Change |
|---|---|
| `current` | Production code |
| `with-topic` | Each state line also carries the topic label |
| `strict` | Criteria say the same label on a different subject, field or city is not a match |
| `topic+strict` | Both |

The pairs cover same activity with different specifics, related work across apps
(writing Dijkstra code vs watching a shortest-path lecture), hard negatives (same
label, different subject or city) and plain unrelated pairs.

## Results

Run 2026-10-24 with `jev-latest`. First number: `match_pairs.json` (24). Second:
`holdout_pairs.json` (12 pairs on subjects no variant mentions).

| Variant | Right at 0.8 | Best threshold | Highest non-match | Lowest match |
|---|---|---|---|---|
| word overlap (no AI) | 50% | 71% at 0.25 | 0.29 | 0.00 |
| `current` | 75% / 58% | 96% at 0.60 / 92% at 0.60 | 0.58 / 0.48 | 0.38 / 0.32 |
| `with-topic` | 83% / 67% | 92% at 0.70 / 83% at 0.50 | 0.67 / 0.73 | 0.43 / 0.32 |
| `strict` | 88% / 75% | 100% at 0.45 / 100% at 0.35 | 0.06 / 0.14 | 0.45 / 0.37 |
| `topic+strict` | 88% / 83% | 100% at 0.45 / 100% at 0.45 | 0.08 / 0.14 | 0.47 / 0.46 |

Takeaways:
- A 0.8 threshold misses real matches with every variant (half of them on the held-out set with `current`).
- `current` can't separate the groups: some non-matches score above some matches.
- `strict` / `topic+strict` leave a wide gap (non-matches ≤ ~0.14, matches ≥ ~0.46) that holds on
  the held-out subjects. Suggested: `topic+strict` with a threshold around 0.3.
- Caveat: 36 pairs, labelled by us, and the `strict` criteria's examples overlap with
  `match_pairs.json`; the held-out set is the fairer check.
