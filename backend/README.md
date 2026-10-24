# work-match backend

FastAPI + Postgres (Supabase). The schema is in [`schema.sql`](schema.sql), and how
matching works is in [`../docs/DATABASE.md`](../docs/DATABASE.md).

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
cd backend
uv venv && uv pip install -r requirements.txt
cp .env.example .env   # set DATABASE_URL (Supabase or a local Postgres)
```

Load the schema and dev user once (Supabase: paste both files into the SQL editor):

```bash
psql "$DATABASE_URL" -f schema.sql -f seed.sql
```

Start the API on port 8000, which is what the Tauri app calls by default:

```bash
set -a; source .env; set +a; .venv/bin/uvicorn app.main:app --reload --port 8000
```

Docs: http://127.0.0.1:8000/docs. To point the app somewhere else, run it with
`VITE_API_URL=https://... yarn tauri dev`.
