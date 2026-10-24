# Activity capture

How the desktop app works out what you're doing, on your own computer. A Chrome
extension reads the tab you're looking at, the Tauri app keeps that text for about 5
minutes, and a local Llama 3.2 model turns it into the two fields the backend stores in
`curr_task`: a `topic` (`LeetCode`) and a one-sentence `task_summary` ("Working through
graph problems on LeetCode"). Raw page text never leaves the laptop.

## Data flow

```
Chrome (extension/)
  active tab of the focused window, on tab switch / page load / every minute
  Readability text, cut to 2000 chars, URL without query or fragment
        │ POST http://127.0.0.1:47321/capture
        ▼
capture server (src-tauri/src/capture_server.rs)
  Origin must be the extension → sharing must be on → body ≤ 64 KB → http(s) URL only
  → keep host + path → scrub title, text and path → skip if text unchanged for that page
        ▼
activity_buffer.sqlite, table captures (src-tauri/src/buffer.rs)
        │ every 5 min (DEMO_MODE=1: every 30 s); first run 60 s after launch
        ▼
summarizer (src-tauri/src/summarizer.rs)
  newest first, one entry per page, ≤ 800 chars each, ≤ ~6000 chars total
  → llama-server on 127.0.0.1:8089 (src-tauri/src/llm.rs), JSON-schema output, temperature 0
  → {shareable, topic, task_summary} → scrub again + checks → ActivitySummary
  → the rows it read are deleted
        ▼
last_summary row + `activity-summary` event + api::on_summary
  → (not built yet) push {task_summary, topic} to the backend, only when shareable
```

The summary looks like this. `source` is `llama`, `fallback` (the model failed twice) or
`idle` (nothing was captured). When `shareable` is false, `topic` and `task_summary` are
always empty.

```json
{ "shareable": true, "topic": "LeetCode",
  "task_summary": "Working through graph problems on LeetCode",
  "source": "llama", "window_minutes": 5, "created_at": 1761300300000 }
```

## What leaves the laptop

Only `{ "task_summary": "...", "topic": "..." }`, and only for shareable summaries
(`api::push_payload`). The push isn't built yet: `api::on_summary` logs the topic,
`shareable` and `source`, and has a `TODO` where the backend call goes. When nothing is
shareable nothing is pushed, and the backend drops the user's `curr_task` row after 11
minutes, so they simply stop being matchable.

## Retention

- **App start:** `captures` is emptied.
- **Sharing turned off:** `captures` and `last_summary` are emptied, and new captures are
  ignored until sharing is back on.
- **After every summary attempt:** exactly the rows it read are deleted
  (`id <= max_id_read`). This happens on success and on fallback.
- **Every 60 s:** rows older than 6 minutes are deleted, whatever else happens.
- The database runs with `PRAGMA secure_delete = ON` (deleted text is overwritten) and
  `journal_mode = DELETE`.
- Only the summarizer reads `captures`, and nothing exports it.

## Privacy protections

- **Extension:** never runs in incognito; only reads `http(s)` pages that aren't on
  `localhost`; drops the query string and fragment; only captures while Chrome is the
  focused app; stores nothing.
- **Capture server:** listens on 127.0.0.1 only. It accepts only requests whose `Origin`
  is the extension, which stops web pages from writing into the buffer. It doesn't stop
  other programs on the same computer, but those could read the SQLite file directly
  anyway. Emails become `[email]`, phone numbers `[phone]`, and runs of 8+ digits
  `[number]` before anything is stored. It logs only the host and the text length.
- **Model server:** `llama-server` listens on 127.0.0.1 with a random API key that
  changes every launch (by default it accepts requests from any web origin), with the
  web UI and `/slots` turned off. Its output is printed only if it crashes while starting,
  before it has seen any page text.
- **Summary checks:** `task_summary` is shown to a matched stranger, so both fields are
  scrubbed again. The summary becomes not shareable if either field contains `@`, `$`, a
  link (`http://`, `https://`, `www.`) or a scrub placeholder. The topic is cut to 3 words
  and 40 characters, the summary to 120 characters.
- **Known gap:** the panel's webview has the SQL plugin's `sql:allow-load` permission, so
  webview code could open `activity_buffer.sqlite` by path. That's fine while the panel only
  renders the app's own content, but keep it in mind before the panel shows anything
  untrusted.

## Model quality (measured)

These numbers come from seven hand-written scenarios run through `llama-server` b11669 with
the spec's system prompt at temperature 0, on a Windows laptop CPU. The scenarios were
mixed tabs, LeetCode, Zillow, LinkedIn jobs, a CS 61B design doc, Gmail plus a bank page,
and a news article.

| Model | Result |
|---|---|
| Llama 3.2 3B Q4_K_M | 5/7 shareable decisions right, 4–6 s per summary. Gmail plus bank was correctly not shareable. Misses: Zillow came back not shareable (the safe direction), and a news article came back shareable. |
| Llama 3.2 1B Q4_K_M | Marked every scenario it was given not shareable, including the work ones. Prompt variants that made it say "shareable" also let Gmail plus bank through, and summaries copied page text such as listing prices, which the `$` check then rejected. Treat 1B as a last resort. |

Things that were tried and not adopted:

- **Putting `shareable` last in the schema.** The model writes the topic and summary first,
  then decides. This helped topics but made the 3B mark Gmail plus bank as shareable
  ("Viewing Chase statement").
- **Few-shot examples in the prompt.** The 1B copied the examples verbatim.
- **Label hints in the prompt** ("Job hunting (job listings or applications)", …). The 3B
  scored 6/7, but the 1B then called Gmail plus bank "Apartment hunting".

The model tends to name the specific LeetCode problem as the topic ("Course Schedule").
Connections are stored per topic, so when the summary mentions LeetCode, the topic is
forced to `LeetCode` (`canonical_topic` in `summarizer.rs`).

## Set it up

1. **llama-server:** build or place the sidecar as described in
   [tauri/src-tauri/binaries/README.md](../tauri/src-tauri/binaries/README.md). On a Mac
   that's `sh tauri/src-tauri/binaries/build-llama-server.sh`.
2. **Model:** download `Llama-3.2-3B-Instruct-Q4_K_M.gguf` (2.0 GB), or the 1B file as a
   lighter fallback, into
   `~/Library/Application Support/com.sultanalzoghaibi.workmatch/models/`
   (Windows: `%APPDATA%\com.sultanalzoghaibi.workmatch\models\`). The links are in the
   binaries README. Instead of that folder you can set `LLAMA_MODEL_PATH`.
3. **Extension:** load `extension/` unpacked in Chrome (see
   [extension/README.md](../extension/README.md)). Its ID must be
   `cifmncfppiahnojgipabfaaifahknecg`.
4. **Run:** `cd tauri && yarn tauri dev`. Then turn on **Share my activity** in the tray
   menu (right-click the globe). It's off by default and the setting is remembered
   (`curr_user_profile.sharing_enabled`).

Only one instance can capture, because the ports are fixed. When you run two instances
(`yarn dev1` and `yarn dev2`), the second one:

- logs `can't listen on 127.0.0.1:47321`, so it receives no captures;
- has its own app data folder (`com.sultanalzoghaibi.workmatch2`), so it doesn't see the
  first instance's models;
- would fail to bind 8089 even if you gave it a model.

Use the demo push buttons in instance 2 instead.

### Demo mode

```sh
DEMO_MODE=1 yarn tauri dev              # macOS
$env:DEMO_MODE=1; yarn tauri dev        # Windows PowerShell
```

The first summary runs after 30 s and then every 30 s, instead of after 60 s and then
every 5 minutes.

## Configuration

| Variable | Default | Effect |
|---|---|---|
| `DEMO_MODE` | unset | `1` = summarize every 30 s |
| `LLAMA_MODEL_PATH` | `<app data>/models/Llama-3.2-3B-Instruct-Q4_K_M.gguf`, then the 1B file | GGUF model to load |
| `ALLOWED_EXTENSION_ID` | `cifmncfppiahnojgipabfaaifahknecg` | extension allowed to post captures |

## For the UI

- Event `activity-summary`: the summary JSON above, once per summary.
- Command `get_last_summary`: the latest summary, or `null`.
- Commands `get_sharing` and `set_sharing({ enabled })`: the same switch as the tray
  checkbox.

## Check it by hand

The buffer is `activity_buffer.sqlite` in the app data folder:

```sh
sqlite3 ~/Library/Application\ Support/com.sultanalzoghaibi.workmatch/activity_buffer.sqlite \
  "select id, source, host, path, length(text) from captures; select json from last_summary;"
```

A capture without the extension's `Origin` is rejected:

```sh
curl -i -X POST http://127.0.0.1:47321/capture -H 'Content-Type: application/json' \
  -d '{"url":"https://leetcode.com/problems/two-sum/","title":"Two Sum","text":"hi"}'   # 403
```

Log lines: `[capture] <host> <n> chars`, `[llm] llama-server ready`,
`[api] summary source=Llama shareable=true topic="LeetCode"`.

### Still to check on a Mac

The pipeline was built and tested on Windows. These parts are macOS-only and haven't been
run yet:

- `build-llama-server.sh` produces a static binary (`otool -L` lists only system
  libraries), and the 3B model answers quickly on Metal.
- After a `tauri dev` rebuild, the next launch logs `stopped a llama-server left over from
  an earlier run` (the pid-file cleanup, Unix only), and only one `llama-server` is running.
- The glass panel and the tray menu still look and behave as before, with the new
  **Share my activity** checkbox.

## Built with Llama

Summaries are generated by Meta's Llama 3.2, used under the
[Llama 3.2 Community License](https://www.llama.com/llama3_2/license/), which asks
products built with it to show "Built with Llama".
