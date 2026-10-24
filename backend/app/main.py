from uuid import UUID

from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from psycopg import sql

from .db import connect
from .schemas import User, UserUpdate

app = FastAPI(title="work-match backend")

# The Tauri webview calls the API directly: Vite dev server in development,
# tauri://localhost (macOS) / http://tauri.localhost (Windows) when bundled.
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://localhost:1420",
        "tauri://localhost",
        "http://tauri.localhost",
    ],
    allow_methods=["*"],
    allow_headers=["*"],
)

USER_COLUMNS = sql.SQL(
    "id, name, school, bio, discord_handle, age_group, country, sharing_enabled, created_at"
)


@app.get("/health")
def health() -> dict:
    with connect() as conn:
        conn.execute("select 1")
    return {"ok": True}


@app.get("/users/{user_id}", response_model=User)
def get_user(user_id: UUID) -> dict:
    with connect() as conn:
        row = conn.execute(
            sql.SQL("select {} from users where id = %s").format(USER_COLUMNS), (user_id,)
        ).fetchone()
    if row is None:
        raise HTTPException(404, "user not found")
    return row


# TODO: once sign-up exists, take the user from the Authorization token
# instead of trusting the id in the path.
@app.patch("/users/{user_id}", response_model=User)
def update_user(user_id: UUID, update: UserUpdate) -> dict:
    # Every UserUpdate field is a users column, so the keys are safe to use
    # as identifiers. name is NOT NULL in the schema, so it can't be cleared.
    changes = update.model_dump(exclude_unset=True)
    if "name" in changes and changes["name"] is None:
        raise HTTPException(422, "name can't be empty")
    if "sharing_enabled" in changes and changes["sharing_enabled"] is None:
        raise HTTPException(422, "sharing_enabled can't be null")
    if not changes:
        return get_user(user_id)

    assignments = sql.SQL(", ").join(
        sql.SQL("{} = %s").format(sql.Identifier(column)) for column in changes
    )
    query = sql.SQL("update users set {} where id = %s returning {}").format(assignments, USER_COLUMNS)
    with connect() as conn:
        row = conn.execute(query, (*changes.values(), user_id)).fetchone()
    if row is None:
        raise HTTPException(404, "user not found")
    return row
