# Topic eval

Measures the topic the summarizer gives an activity: the label shown to a matched
stranger, sent to Jev, and used to store connections. It runs the app's real
`summarize()` (prompt, cleanup, topic rules) from `src-tauri/src/topic_eval.rs`
against a llama-server with the app's model.

```sh
# 1. a llama-server on 8095 with the app's model
tauri/src-tauri/binaries/llama-server-aarch64-apple-darwin --port 8095 -c 4096 --no-webui \
  -m ~/Library/Application\ Support/com.sultanalzoghaibi.workmatch/models/Llama-3.2-3B-Instruct-Q4_K_M.gguf

# 2. the eval (ignored by plain `cargo test`)
cd tauri/src-tauri
TOPIC_EVAL_LABEL=mine cargo test topic_eval -- --ignored --nocapture
TOPIC_EVAL_CASES=holdout.json TOPIC_EVAL_LABEL=mine-holdout cargo test topic_eval -- --ignored --nocapture
```

Per-case results go to `results-<label>.json` (gitignored).

## Cases

`make_cases.py` builds both files.

- **`cases.json` (26, used for tuning):** Sultan's 16 realistic cases from
  `../summarizer-eval/cases.py` (imported, not edited), plus 10 "tool noise" cases:
  real work with `yarn`, `npm`, `git`, `pip`, `docker`, an editor or a terminal on screen.
- **`holdout.json` (10):** written and committed before any tuning, and run once at the end.

A case passes when the shareable decision is right, the topic is one of `good_topics`,
and the topic contains none of `bad_words` (tool, app and site names). `unsure` cases are
judgment calls, so they're shown but not counted.

## Results (2026-10-24, Llama 3.2 3B Q4_K_M)

| | Before | Fixed topic list | + focus |
|---|---|---|---|
| Tuning cases (24 counted) | 4/24 (17%) | 22/24 (92%) | 21/24 (88%) |
| **Held-out cases (10)** | 4/10 (40%) | 7/10 (70%) | **8/10 (80%)** |
| Topics naming a tool or site (tuning + held-out) | 2 + 2 | 0 + 0 | 0 + 0 |
| Usable focus (tuning / held-out) | – | – | 22/24 · **10/10** |

The held-out set was run once per version, after that version's tuning, and never
tuned on. Differences of one case are within the model's run-to-run noise.

**Before**, the prompt's example sentence ("Working through graph problems on LeetCode")
was copied into unrelated summaries; for example, a hackathon backend got topic "LeetCode".
Topics were page-title fragments ("Google Maps", "Group by", "SQL Databases") or tools
("Git rebase", "Tauri").

**What changed in `summarizer.rs`:**

1. **The topic comes from a fixed list**, enforced by the JSON schema: Coding, Web
   development, App development, Data science, Machine learning, Design, Figma design,
   Writing, Research, Studying, Coursework, LeetCode, Job hunting, Apartment hunting,
   Shopping, or Other. "Other" is never shared.
2. **A rewritten prompt** with a one-line definition per label and no example sentence
   to copy. It says to describe the goal, not the page or the tools on screen.
3. **Course codes come from the titles on screen**, not the model ("CS 61B", "Bio 101",
   "Physics 7B"). When a course is visible, course-type work gets the course as its topic.
   When Llama was allowed to write course codes itself, it made them up ("MIGR 202X").
4. **LeetCode on screen** turns a coding or studying topic into "LeetCode", because the
   model is led by problem names ("Course Schedule" → Coursework).

5. **A second field, `focus`**: 2 to 4 words naming the specific subject inside the
   topic ("Coding" + "desktop matching app", "Coursework" + "pendulum period"). A focus
   that names a tool (`yarn`, `git`, a terminal, an editor) or repeats the topic is
   dropped, and the summary is still shared. The app sends it with each push; the backend
   ignores it until its push model has a `focus` field.

**Still wrong on held-out:** Spanish practice → Writing, a resume plus job listings →
Writing (should be Job hunting), CS 70 probability homework → Web development. These
weren't tuned on, to keep the held-out score honest. They're good next cases to work on
with a fresh held-out set.

Summaries (not scored) still sometimes name a tool ("with Next.js", "in Unity").
