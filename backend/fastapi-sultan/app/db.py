import os
from contextlib import contextmanager
from pathlib import Path

import psycopg
from dotenv import load_dotenv
from psycopg.rows import dict_row

# backend/fastapi-sultan/.env, wherever uvicorn is started from.
load_dotenv(Path(__file__).resolve().parent.parent / ".env")


def database_url() -> str:
    url = os.environ.get("DATABASE_URL")
    if not url:
        raise RuntimeError("DATABASE_URL is not set (see backend/fastapi-sultan/.env.example)")
    return url


@contextmanager
def connect():
    """One short-lived connection per request.

    Fine for the hackathon and for serverless hosts. prepare_threshold=None
    keeps it working behind Supabase's transaction pooler (port 6543).
    """
    with psycopg.connect(database_url(), row_factory=dict_row, prepare_threshold=None) as conn:
        yield conn
