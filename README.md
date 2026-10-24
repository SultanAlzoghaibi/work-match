# work-match

## The problem today

Working alone is isolating. Somewhere, someone in your field is working right now too, but finding them takes effort: posting in a Discord, asking in a class chat, going to meetups. Most people don't, so they grind alone. Even when they do find someone, it's by luck, and it rarely happens while they're actually working.

## What work-match fixes

You do nothing extra. As you work, a background app notices what you're on (apps, sites and calendar) and finds someone in your field who's working right now, live. You get one low-pressure intro:

> "PERSON A is also doing LeetCode right now. Want to work alongside her?"

If it clicks, add her as a connection and join a call with her (or a Discord call, to save time and not reinvent the wheel). From then on, the app tells you whenever she's working on that topic, so you can join her.

## Architecture

```
Tauri app (on your Mac)                          Backend (Supabase)                Jev
───────────────────────                          ──────────────────                ───
reads current app, URLs, page HTML
  → filters out private stuff (blocklist)
  → local AI summarizes the last 5 min
every 5 min: PUSH task_summary + topic ───────→  curr_task (overwrite) ────────→  scores me against
                                                 is_working_on_connections         filtered candidates
                                                 matches  ←──────── good pair ───  (age, country, ...)
every 20 s: POLL ←─────────────────────────────  pending matches + active friends
  → match popup / friend "active" icon
quit or pause ────────────────────────────────→  delete my curr_task + active rows
```

- **Push (every 5 min):** the app sends only a summary and a topic label. Raw activity
  never leaves the device. Each push also **triggers the Jev search for that user**.
- **Poll (every 20 s):** the app asks for new match popups and which friends are active.
- **Quit or pause:** the app deletes its `curr_task`, so nobody gets matched with or
  pinged about someone who isn't working.
- **Match** = a one-time pairing that leads to a call. **Connection** = a permanent
  follow on a topic. See [docs/DATABASE.md](docs/DATABASE.md) and the full walkthrough in
  [docs/USE_CASE.md](docs/USE_CASE.md).

## Repo layout

- `backend/` – matching server and database schema (`schema.sql`)
- `tauri/` – macOS menu-bar (tray) desktop app
- `extension/` – Chrome extension that sends the active tab's text to the desktop app
  (see [docs/activity-capture.md](docs/activity-capture.md))

## Platforms

- **macOS (main target):** menu-bar only app (no dock icon). Clicking the tray globe opens a native **Liquid Glass** panel on macOS 26+, with a vibrancy blur fallback on older macOS.
- **Windows (supported, not the focus):** same tray app. The window uses a CSS semi-transparent background instead of native glass.

## Run the desktop app

```bash
cd tauri
yarn
yarn dev1   # instance 1 (same as yarn tauri dev)
```

### Two instances on one Mac (to test matching)

Each instance is a separate person with its own port, local SQLite and backend user:

| | Instance 1 | Instance 2 |
|---|---|---|
| Command | `yarn dev1` | `yarn dev2` |
| Vite dev port | 1420 | 1421 |
| Local SQLite | `~/Library/Application Support/com.sultanalzoghaibi.workmatch/work_match.db` | `~/Library/Application Support/com.sultanalzoghaibi.workmatch2/work_match.db` |
| Backend user (`MOCK_USER_ID`) | `00000000-0000-0000-0000-000000000000` | `00000000-0000-0000-0000-000000000001` |
| Rust build dir | `src-tauri/target` | `src-tauri/target-instance2` |

Run them in two terminals. Instance 2's settings live in `src-tauri/tauri.conf.instance2.json`; its first build takes a few minutes because it compiles into its own folder. You'll see two globes in the menu bar. Both use the same FastAPI on port 8000, so seed both users with `backend/seed.sql` first.

Left-click the smiling globe in the menu bar to open the app window. Closing the window keeps work-match running in the menu bar; right-click → Quit to exit.

## Run the backend

Deployed (Vercel, same Supabase): `https://work-match-api.vercel.app`. To point the app at it
instead of a local FastAPI, start it with the URL:

```bash
cd tauri && yarn prod     # instance 1 against the deployed API (prod2 for instance 2)
```

See [backend/fastapi-sultan/README.md](backend/fastapi-sultan/README.md) (owned by Sultan: don't edit that folder). The profile Edit form saves to the local SQLite first, then `PATCH /users/{id}` on the FastAPI backend at `http://127.0.0.1:8000`.

See [docs/DESIGN.md](docs/DESIGN.md) for the design doc. Activity capture (Chrome
extension, local buffer, local Llama summaries) needs a sidecar binary, a model file and
the extension: see [docs/activity-capture.md](docs/activity-capture.md). Summaries are
built with Llama.

## What activity capture sends to the backend

It sends only this body, and only when the local model marks the summary as shareable:

```json
{ "task_summary": "Working through graph problems on LeetCode", "topic": "LeetCode" }
```

- `task_summary`: one sentence, at most 120 characters. The person you're matched with
  sees it.
- `topic`: a 1–3 word label (`LeetCode`, `Job hunting`, `CS 61B`, …). It's used for
  connections and the "friend is active" icon.

Nothing else leaves the laptop: no URLs, page text, titles or timestamps. When a summary
isn't shareable, nothing is sent. `api::on_summary` in `tauri/src-tauri/src/api.rs` sends
it as the body of `POST /users/{id}/push` (see
[backend/fastapi-sultan/README.md](backend/fastapi-sultan/README.md)).
