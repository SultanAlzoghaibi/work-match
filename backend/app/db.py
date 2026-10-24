"""Postgres access. DATABASE_URL is the Supabase connection string (use the
pooler URI on hosts without IPv6, e.g. Render)."""

from collections.abc import Iterator
from contextlib import contextmanager

import psycopg
from psycopg.rows import dict_row
from psycopg_pool import ConnectionPool

_pool: ConnectionPool | None = None


def open_pool(database_url: str) -> None:
    global _pool
    # prepare_threshold=None: Supabase's transaction pooler can't hold prepared statements.
    _pool = ConnectionPool(
        database_url,
        min_size=1,
        max_size=5,
        kwargs={"row_factory": dict_row, "prepare_threshold": None},
        open=True,
    )


def close_pool() -> None:
    global _pool
    if _pool is not None:
        _pool.close()
        _pool = None


@contextmanager
def connection() -> Iterator[psycopg.Connection]:
    """One transaction: commits on success, rolls back on error."""
    if _pool is None:
        raise RuntimeError("database pool is not open")
    with _pool.connection() as conn:
        yield conn
