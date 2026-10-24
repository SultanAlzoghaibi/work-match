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

| Set | `main` | This branch |
|---|---|---|
| Tuning cases (`cases.json`, 24 counted) | 4/24 (17%) | **22/24 (92%)** |
| Held-out (`holdout.json`, 10) | 4/10 (40%) | **10/10** |
| Off-list #1–#3 (`offlist.json`, 16) | – | **14/16** |
| Final, written last and run once (`offlist.json`, 6) | – | **5/6** (the miss is "Crafting" for pottery, which isn't in the accepted list but is right) |
| Topics naming a tool or site, all sets | 4 | **0** |
| Usable `focus`, all sets | – | **65/66** |

The held-out set and off-list #1–#3 were each written before the change they checked,
but later rounds were tuned while looking at them, so their scores are optimistic. The
final 6 are the honest check.

## How topics are picked now (`summarizer.rs`)

1. **`activity`**: the model first names the general field or hobby in its own words
   (1–2 words, "as you'd name a school club for it").
2. **`topic`**: then it picks a label from the list (Coding, Web development, App
   development, Data science, Machine learning, Design, Figma design, Writing, Research,
   Studying, Coursework, Music, LeetCode, Job hunting, Apartment hunting, Shopping) or
   Other. The JSON schema enforces the list.
3. **`reconcile_topic`** decides between the two. If the model's own words are a label,
   a known synonym ("Real estate" → Apartment hunting, "Programming" → Coding), or share
   a word stem with the label ("Ui design" / Design), the label is kept. If they're
   unrelated ("Knitting" vs Writing), the model's own words win, because with a list in
   front of it a small model reaches for the nearest label. Other always uses the own words.
4. **Screen rules run first:** a course code in a window or page title ("CS 61B", "Bio 1A")
   becomes the topic for anything except job, apartment or shopping hunting, and LeetCode
   on screen means LeetCode.
5. **Tools never become the topic:** "Yarn", "Git rebase" and the like become Coding.
6. **`focus`**: 2 to 4 words naming the specific subject ("desktop matching app",
   "pendulum period calculation"). It's shown to a match and sent with each push, but
   people don't need the same focus to match. A focus that names a tool is dropped.
7. **The prompt** defines each label and has no example sentence to copy. The old one's
   "Working through graph problems on LeetCode" got copied into unrelated summaries.

**Known limits:** occasional label mix-ups (a SwiftUI app → Web development, chess study →
Studying), and own words that are sometimes too specific ("Noise reduction" for podcast
editing).
