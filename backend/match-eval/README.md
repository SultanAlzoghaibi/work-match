# Match eval

Measures how well Jev decides whether two people should be matched, on hand-labelled
pairs of summarizer outputs (`match_pairs.json`, same `{topic, task_summary}` shape as
`docs/activity-capture.md`). Use it before changing the Jev question or the threshold.

```sh
cd backend/match-eval
../.venv/bin/python run_eval.py                 # all variants (needs JEV_API)
../.venv/bin/python run_eval.py --variant current
../.venv/bin/python run_eval.py --offline       # word-overlap baseline, no key
../.venv/bin/python run_eval.py --no-calls      # only cached answers, never calls Jev
```

Jev answers are cached in `.jev_cache.json` (gitignored), keyed by the exact request, so
re-runs only call Jev for new requests.

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
| `general` | Topic + criteria as general rules, with no subjects, places or jobs from the pair files (the fair version of `topic+strict`) |

The pairs cover same activity with different specifics, related work across apps
(writing Dijkstra code vs watching a shortest-path lecture), hard negatives (same
label, different subject or city) and plain unrelated pairs.

## Results

Final run, 2026-10-24, `jev-latest`: 68 counted pairs across `match_pairs.json` (24),
`holdout_pairs.json` (12) and `extra_pairs.json` (32, 5 more marked `unsure` and not
counted), plus 5 ranking groups (one person vs 5 candidates in one request, like
production). Re-scored at each threshold from `.jev_cache.json` with no extra calls.

| | Right (68 pairs) | Ranking: top pick right | Ranking: every candidate right |
|---|---|---|---|
| word overlap, no AI (best threshold) | 72% | – | – |
| **`current` at 0.8 (production today)** | **62%** | **1/5** | 0/5 |
| `current` at its best threshold (0.35) | 84% | 5/5 | 4/5 |
| `general` at 0.8 | 84% | 4/5 | 3/5 |
| **`general` at 0.45** | **93%** | **5/5** | **5/5** |

- **Production misses most matches today.** At 0.8, 4 of the 5 ranking groups get no match
  at all, including Dijkstra code vs a shortest-path lecture (0.73–0.78).
- **`general` + threshold 0.45 is the recommended setting**: best on every file (96% /
  100% / 91%) and on ranking.
- **Still wrong with `general` at 0.45 (5):** same goal in a different area scores low
  (coding vs behavioral interview prep 0.18, interview prep vs system design 0.34, contracts
  vs torts 0.22), and the same task in a different field or city scores high (cover letters
  for marketing vs software 0.75, apartments in SF vs Brooklyn 0.65). Several of these are
  judgment calls in our labels.
- **The earlier `topic+strict` 100% was inflated.** Its criteria named cities and subjects
  from the pair files: SF vs Brooklyn apartments scored 0.08 with it and 0.65 with
  `general`, which names none.
- Jev scores move a little between runs (e.g. 0.58 → 0.65 for the same pair), so treat
  differences under ~0.1 as noise.

Earlier runs (24 + 12 pairs, before `general` and the extra pairs) are in git history.
