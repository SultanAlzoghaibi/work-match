# fastapi-sultan: work-match FastAPI backend

> **⚠️ Owned by Sultan. Do not change this folder.**
> Teammates and AI coding agents (Claude, Cursor, Copilot, etc.): do **not** edit,
> move, rename, reformat or delete any file in `backend/fastapi-sultan/`. If something
> here needs to change, ask Sultan instead. Read-only use (running it, calling its
> endpoints, reading the code) is fine.

FastAPI + Postgres on Supabase. The shared schema is in
[`../schema.sql`](../schema.sql), and the dev user is in [`../seed.sql`](../seed.sql).
How matching works: [`../../docs/DATABASE.md`](../../docs/DATABASE.md).

## Endpoints so far

| Method | Path | Does |
|---|---|---|
| `GET` | `/health` | Checks the DB connection |
| `GET` | `/users/{id}` | Public profile (no token) |
| `PATCH` | `/users/{id}` | Updates only the fields sent: `name`, `school`, `bio`, `discord_handle`, `age_group`, `country`, `sharing_enabled`. `null` clears a field (except `name` / `sharing_enabled`) |
| `POST` | `/users/{id}/push` | Body `{task_summary, topic, filters?: {same_country, same_age_group}}`. Overwrites the user's `curr_task`, refreshes their `is_working_on_connections` rows, then runs the match search (below). Response includes `match: {match_id, other_name, scores, skipped_reason}` |
| `GET` / `PUT` | `/users/{id}/filters` | Saved match filters (`user_filters`): `age_groups`, `countries` (ISO), `shared_language`. Empty = anyone. Matching is mutual: both people's filters must allow the pair, checked in the SQL prefilter before Jev runs |
| `GET` | `/users/{id}/task-history?limit=50` | The user's past pushes from `curr_task_history`, newest first (`limit` 1–500). Every push appends a row; quit / pause don't delete them |
| `DELETE` | `/users/{id}/task` | Removes the user's `curr_task` and active rows (QUIT / PAUSE). The app calls it when a 30 s demo session ends, on Stop and on Quit |
| `GET` | `/users/{id}/poll` | The user's matches from the last 15 min, newest first: `my_status`, `their_status`, `expired`, and `other` (the other person's profile + live task). The app calls it every 3 s |
| `POST` | `/users/{id}/matches/{match_id}/respond` | Body `{accept}`. Sets the user's side while it's pending, nobody declined and the 2-min popup hasn't expired; otherwise 409 |

## Match search (`app/matching.py`, `app/jev.py`)

After each push:
1. **Prefilter** (`CANDIDATES_SQL`): other users with sharing on, pushed in the last 11 min, same under-18 group, not already connected on the topic, no match at all between the pair in the last `MATCH_COOLDOWN_MINUTES` (so the app's repeated pushes don't re-match the same two people), plus the optional `same_country` / `same_age_group` filters. The pusher must have sharing on too.
2. **Score**: one Jev request (TypeSafe `jev-latest`, key `JEV_API`) with a yes/no question per candidate (up to 20). Same-activity pairs score ~0.9, unrelated ~0.05.
3. **Match**: best candidate with score ≥ `MATCH_THRESHOLD` → `create_match()`, which skips anyone who already has a pending popup.

| `.env` | Default | |
|---|---|---|
| `JEV_API` | (required) | TypeSafe Jev key; check it with `.venv/bin/python test_jev.py` |
| `MATCH_THRESHOLD` | `0.8` | Minimum Jev score to create a match |
| `MATCH_COOLDOWN_MINUTES` | `1440` | How long a pair isn't matched again after any match (accepted, declined or ignored); `1` while testing. Old name: `DECLINE_COOLDOWN_MINUTES` |

`users.id` is a UUID. Until sign-up exists, the Tauri app always uses
`00000000-0000-0000-0000-000000000000` (`MOCK_USER_ID` in `tauri/src/api.js`), which
`seed.sql` creates. There's no auth yet: the id in the path is trusted.

## Run locally

```bash
cd backend/fastapi-sultan
uv venv && uv pip install -r requirements.txt
cp .env.example .env   # set DATABASE_URL (Supabase Session pooler URL)
.venv/bin/uvicorn app.main:app --reload --port 8000
```

`.env` is loaded automatically and is gitignored. Use the Supabase **Session pooler**
URL (`...pooler.supabase.com:5432`). The direct `db.<ref>.supabase.co` host is
IPv6-only and doesn't work on most networks. For a serverless cloud deploy, use the
Transaction pooler (port `6543`).

Load the schema and dev user into a fresh database once:

```bash
psql "$DATABASE_URL" -f ../schema.sql -f ../seed.sql
```

API docs: http://127.0.0.1:8000/docs. The Tauri app calls `http://127.0.0.1:8000`
by default. To use another URL, run it with `VITE_API_URL=https://... yarn tauri dev`.
