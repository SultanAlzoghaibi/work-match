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
| `general-no-place` | `general` without "place" in the false rule, plus "Being in a different city or at a different event doesn't matter" |
| `general-agnostic` | `general-no-place` that also says a different company doesn't matter |

The pairs cover same activity with different specifics, related work across apps
(writing Dijkstra code vs watching a shortest-path lecture), hard negatives (same
label, different subject or city) and plain unrelated pairs.

## Results

### Different company (2026-10-24, `company_pairs.json` added)

11 new pairs (10 counted): the same marketing, sales, engineering, data, recruiting or
onboarding work at different companies (match), and the same company with a different role
(no match). 121 pairs total, 105 counted, threshold 0.45:

| Variant | Right | New pairs | Company matches score | Same company, different role |
|---|---|---|---|---|
| `current` (production) | 94% | 100% | 0.62–0.88 | 0.18, 0.24 |
| `general-no-place` | 96% | 100% | | |
| `general-agnostic` | 96% (97% at 0.50) | 100% | 0.69–0.92 | 0.06, 0.24 |

Matching is already company-agnostic; saying so in the criteria widens the margin both
ways. B2B whitepaper vs consumer TikTok marketing scores ~0.1 with every wording (left
`unsure`: same function, very different work).

### Different city / event (2026-10-24, `place_event_pairs.json` added)

13 new pairs (11 counted): the same apartment, job, car or course search in a different
city or school, the same kind of hackathon work at a different event, and hard negatives in
the same city or at the same event. 110 pairs total, 95 counted, threshold 0.45:

| Variant | Right | Missed matches | Wrong matches | New pairs | Ranking |
|---|---|---|---|---|---|
| `current` (production = `general`) | 94% | 4 | 2 | 100% | 5/5 · 5/5 |
| `general-no-place` | **96%** | 2 | 2 | 100% | 5/5 · 5/5 |

Production already matches different cities and events; dropping "place" fixes two misses
(Japan trip vs hiragana, video grading vs vlog) and lifts Chicago-vs-Austin from 0.17 to
0.37, while the "same task, different field" mistakes score slightly higher. Same event,
different project stays unmatched (~0.20) with both. Jev varies ~0.1 between runs, so the
edge is small.

### Final, with the team's labels (2026-10-24)

Team decisions on the judgment calls: the same task for a different field is **not** a
match (cover letters, resumes); the same search in a different place **is** a match
(apartments, used cars); and the same goal on a different skill or class (coding vs
behavioral interview prep, contracts vs torts) depends on the **user's filters**, so those
pairs are marked `unsure` and not scored. 84 pairs counted, 13 not.

| Setup | Right (84 pairs) | Real matches missed | Ranking: top / every candidate |
|---|---|---|---|
| **Production today: current wording, 0.8** | **58%** | **33 of 46** | 1/5 · 0/5 |
| Current wording at its best threshold (0.20) | 85% | | 5/5 · 2/5 |
| **Proposed: `general` + topic, 0.45** | **96%** | **1 of 46** | **5/5 · 5/5** |
| `two-product`, 0.15 | 93% | | 5/5 · 4/5 |
| `two-mean`, 0.25 | 94% | | 5/5 · 3/5 |

`general` is also 97% on the older pairs and 94% on the 18 fresh ones (threshold picked
on the older pairs only). Its 3 mistakes: cover letters marketing vs software (0.75),
resumes nursing vs accounting (0.62), Chicago rentals vs Austin condos (0.26).

### Before the team's labels

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

### Two-question variants (scored fairly)

`fresh_pairs.json` (24) was written and committed before the two-question variants
existed. Each variant's threshold was picked on the other 68 pairs only, then graded on
the fresh 24.

| Variant | Threshold (from the 68) | 68 pairs | Fresh 24 | Ranking: top / every candidate |
|---|---|---|---|---|
| `current` | 0.35 | 84% | 79% | 5/5 · 4/5 |
| **`general`** | 0.45 | **93%** | 88% | **5/5 · 5/5** |
| `two-product` | 0.15 | 91% | **96%** | 5/5 · 4/5 |
| `two-mean` | 0.25 | 93% | 92% | 5/5 · 3/5 |
| `two-goal` / `two-min` | 0.20 | 88–90% | 96% | 4–5/5 · 3/5 |

- The second question fixes "same task, different field" on fresh pairs (nursing vs
  accounting resumes), but gives a little back elsewhere; over all 92 pairs it's ~92% vs
  ~91%, within Jev's run-to-run noise. Its 0.15 threshold is also fragile.
- Recommendation stays **`general` at 0.45**; `two-product` is an option if avoiding
  wrong-field matches matters most.
- Every variant matches used cars in Denver vs Miami (same task, different city).

Earlier runs (24 + 12 pairs, before `general` and the extra pairs) are in git history.
