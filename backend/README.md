# Backend

FastAPI server in `app/`, on Supabase (Postgres) with the schema in `schema.sql`.
Endpoints are documented in [docs/API.md](../docs/API.md); the data model in
[docs/DATABASE.md](../docs/DATABASE.md).

## Run locally

```sh
cd backend
python3.12 -m venv .venv && .venv/bin/pip install -r requirements.txt
cp .env.example .env        # fill in DATABASE_URL
.venv/bin/uvicorn app.main:app --reload    # http://127.0.0.1:8000/docs
```

## Tests

`tests/test_api.py` runs the whole flow against a scratch Postgres that has
`schema.sql` loaded. It empties every table first, so never point it at Supabase.

```sh
TEST_DATABASE_URL=postgresql://postgres:postgres@127.0.0.1:54322/wm_api_test \
  .venv/bin/python -m unittest discover tests -v
```

## Deploy (Google Cloud Run)

The server is a plain container (`Dockerfile`), so it also runs on AWS App
Runner/ECS or Fly without changes. It stores nothing; all data is in Supabase.

```sh
gcloud run deploy work-match-api --source backend --region us-west1 \
  --allow-unauthenticated --set-env-vars DATABASE_URL=...
```
