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
