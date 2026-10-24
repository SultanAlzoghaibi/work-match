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

| Variant | Accuracy at 0.8 | Best threshold | Notes |
|---|---|---|---|
| word overlap (no AI) | 50% | 71% at 0.25 | Misses every cross-app and different-wording pair |
