from uuid import UUID

from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from psycopg import errors, sql

from .db import connect
from .matching import find_match
from .schemas import CurrTask, MatchView, RespondBody, TaskPush, User, UserUpdate

app = FastAPI(title="work-match backend")

# The Tauri webview calls the API directly: Vite dev server in development,
# tauri://localhost (macOS) / http://tauri.localhost (Windows) when bundled.
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://localhost:1420",  # instance 1 (yarn dev1)
        "http://localhost:1421",  # instance 2 (yarn dev2)
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


# PUSH (schema.sql): overwrite my curr_task, rewrite my
# is_working_on_connections rows for connections on this topic, then run the
# Jev match search for me (matching.py).
@app.post("/users/{user_id}/push", response_model=CurrTask)
def push_task(user_id: UUID, task: TaskPush) -> dict:
    topic = task.topic or "General"
    try:
        with connect() as conn:
            row = conn.execute(
                """
                insert into curr_task (user_id, task_summary, topic) values (%s, %s, %s)
                on conflict (user_id) do update set
                    task_summary = excluded.task_summary,
                    topic = excluded.topic,
                    pushed_at = now()
                returning user_id, task_summary, topic, pushed_at
                """,
                (user_id, task.task_summary, task.topic),
            ).fetchone()
            conn.execute("delete from is_working_on_connections where user_id = %s", (user_id,))
            conn.execute(
                """
                insert into is_working_on_connections (connection_id, user_id)
                select id, %s from connections where %s in (user_a, user_b) and topic = %s
                """,
                (user_id, user_id, task.topic),
            )
            conn.commit()
            match = find_match(conn, user_id, task.task_summary, topic, task.filters)
    except errors.ForeignKeyViolation:
        raise HTTPException(404, "user not found") from None
    print(f"[push] {user_id} topic={topic!r} -> {match}")
    return {**row, "match": match}


MATCH_VIEW_SQL = """
select m.id as match_id, m.topic, m.score, m.created_at,
       case when m.user_a = %(me)s then m.a_status else m.b_status end as my_status,
       case when m.user_a = %(me)s then m.b_status else m.a_status end as their_status,
       ('declined' not in (m.a_status, m.b_status)
        and 'pending' in (m.a_status, m.b_status)
        and m.created_at < now() - interval '2 minutes') as expired,
       u.id as other_id, u.name, u.school, u.bio, u.age_group, u.country, u.discord_handle,
       t.task_summary
from matches m
join users u on u.id = case when m.user_a = %(me)s then m.user_b else m.user_a end
left join curr_task t on t.user_id = u.id
where %(me)s in (m.user_a, m.user_b)
"""


def _match_view(row: dict) -> dict:
    other = {
        "id": row["other_id"],
        **{key: row[key] for key in ("name", "school", "bio", "age_group", "country",
                                     "discord_handle", "task_summary")},
    }
    view = {key: row[key] for key in ("match_id", "topic", "score", "created_at",
                                      "my_status", "their_status", "expired")}
    return {**view, "other": other}


# POLL (every ~3 s from the app): my matches from the last 15 minutes, newest
# first, with both answers so the popup can show waiting / accepted / declined.
@app.get("/users/{user_id}/poll", response_model=list[MatchView])
def poll(user_id: UUID) -> list[dict]:
    with connect() as conn:
        rows = conn.execute(
            MATCH_VIEW_SQL
            + " and m.created_at > now() - interval '15 minutes' order by m.created_at desc limit 5",
            {"me": user_id},
        ).fetchall()
    return [_match_view(row) for row in rows]


# ACCEPT / DECLINE: set my side only, while I haven't answered, nobody has
# declined and the 2-minute popup hasn't expired.
@app.post("/users/{user_id}/matches/{match_id}/respond", response_model=MatchView)
def respond(user_id: UUID, match_id: int, body: RespondBody) -> dict:
    status = "accepted" if body.accept else "declined"
    with connect() as conn:
        updated = conn.execute(
            """
            update matches set
                a_status = case when user_a = %(me)s then %(status)s else a_status end,
                b_status = case when user_b = %(me)s then %(status)s else b_status end
            where id = %(id)s
              and %(me)s in (user_a, user_b)
              and (case when user_a = %(me)s then a_status else b_status end) = 'pending'
              and 'declined' not in (a_status, b_status)
              and created_at > now() - interval '2 minutes'
            returning id
            """,
            {"me": user_id, "status": status, "id": match_id},
        ).fetchone()
        row = conn.execute(MATCH_VIEW_SQL + " and m.id = %(id)s", {"me": user_id, "id": match_id}).fetchone()
    if row is None:
        raise HTTPException(404, "match not found")
    if updated is None:
        raise HTTPException(409, "this match can no longer be answered")
    return _match_view(row)
