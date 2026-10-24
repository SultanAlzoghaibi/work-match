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

## Platforms

- **macOS (main target):** menu-bar only app (no dock icon). Clicking the tray globe opens a native **Liquid Glass** panel on macOS 26+, with a vibrancy blur fallback on older macOS.
- **Windows (supported, not the focus):** same tray app. The window uses a CSS semi-transparent background instead of native glass.

## Run the desktop app

```bash
cd tauri
yarn
yarn tauri dev
```

Left-click the smiling globe in the menu bar to open the app window. Closing the window keeps work-match running in the menu bar; right-click → Quit to exit.

## Run the backend

See [backend/README.md](backend/README.md). The profile Edit form saves to the local SQLite first, then `PATCH /users/{id}` on the FastAPI backend at `http://127.0.0.1:8000`.

See [docs/DESIGN.md](docs/DESIGN.md) for the design doc.
