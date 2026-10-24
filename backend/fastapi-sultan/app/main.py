from uuid import UUID

import httpx
from fastapi import FastAPI, File, HTTPException, Query, Response, UploadFile
from fastapi.middleware.cors import CORSMiddleware
from psycopg import errors, sql

from .db import connect
from .matching import (
    active_connections,
    clear_expired_pending,
    find_match,
    list_connections,
    refresh_active_connections,
)
from .storage import upload_avatar
from .schemas import (
    Connection,
    CurrTask,
    KeepBody,
    MatchView,
    RespondBody,
    TaskHistoryItem,
    TaskPush,
    User,
    UserUpdate,
)

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
    "id, name, school, career, bio, discord_handle, avatar_url, age_group, country, sharing_enabled, created_at"
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


AVATAR_TYPES = {"image/jpeg", "image/png", "image/webp"}
AVATAR_MAX_BYTES = 2 * 1024 * 1024


# PROFILE PICTURE: the file goes to Supabase Storage (bucket "avatars"), and
# users.avatar_url stores its public URL. The app resizes to 256x256 first.
@app.post("/users/{user_id}/avatar", response_model=User)
def set_avatar(user_id: UUID, file: UploadFile = File(...)) -> dict:
    if file.content_type not in AVATAR_TYPES:
        raise HTTPException(415, "use a JPEG, PNG or WebP image")
    data = file.file.read(AVATAR_MAX_BYTES + 1)
    if len(data) > AVATAR_MAX_BYTES:
        raise HTTPException(413, "image is over 2 MB")
    get_user(user_id)   # 404 before uploading for a user that doesn't exist
    try:
        avatar_url = upload_avatar(user_id, data, file.content_type)
    except httpx.HTTPError as err:
        raise HTTPException(502, f"storage upload failed: {err}") from None
    with connect() as conn:
        row = conn.execute(
            sql.SQL("update users set avatar_url = %s where id = %s returning {}").format(USER_COLUMNS),
            (avatar_url, user_id),
        ).fetchone()
    return row


@app.delete("/users/{user_id}/avatar", response_model=User)
def clear_avatar(user_id: UUID) -> dict:
    with connect() as conn:
        row = conn.execute(
            sql.SQL("update users set avatar_url = null where id = %s returning {}").format(USER_COLUMNS),
            (user_id,),
        ).fetchone()
    if row is None:
        raise HTTPException(404, "user not found")
    return row


# PUSH (schema.sql): overwrite my curr_task, rewrite my
# is_working_on_connections rows for connections whose topic Jev says matches
# this task, then run the Jev match search for me (matching.py).
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
            # History: every push, kept after quit / pause (curr_task_history).
            conn.execute(
                "insert into curr_task_history (user_id, task_summary, topic, pushed_at) values (%s, %s, %s, %s)",
                (user_id, row["task_summary"], row["topic"], row["pushed_at"]),
            )
            conn.commit()
            # Friends first: light up connections whose topic matches this task,
            # then look for a new person.
            connection_topics = refresh_active_connections(conn, user_id, task.task_summary)
            conn.commit()
            match = find_match(conn, user_id, task.task_summary, topic, task.filters)
    except errors.ForeignKeyViolation:
        raise HTTPException(404, "user not found") from None
    print(f"[push] {user_id} topic={topic!r} -> {match} connections={connection_topics}")
    return {**row, "match": match, "connection_topics": connection_topics}


# QUIT / PAUSE (schema.sql): stop being matchable. The app calls this when a
# task session ends, on pause and on quit.
@app.delete("/users/{user_id}/task", status_code=204)
def clear_task(user_id: UUID) -> Response:
    with connect() as conn:
        conn.execute("delete from curr_task where user_id = %s", (user_id,))
        conn.execute("delete from is_working_on_connections where user_id = %s", (user_id,))
    return Response(status_code=204)


MATCH_VIEW_SQL = """
select m.id as match_id, m.topic, m.score, m.created_at,
       case when m.user_a = %(me)s then m.a_status else m.b_status end as my_status,
       case when m.user_a = %(me)s then m.b_status else m.a_status end as their_status,
       ('declined' not in (m.a_status, m.b_status)
        and 'pending' in (m.a_status, m.b_status)
        and m.created_at < now() - interval '2 minutes') as expired,
       case when m.user_a = %(me)s then m.a_keep else m.b_keep end as my_keep,
       case when m.user_a = %(me)s then m.b_keep else m.a_keep end as their_keep,
       exists (select 1 from connections c
               where c.user_a = m.user_a and c.user_b = m.user_b and c.topic = m.topic) as connected,
       u.id as other_id, u.name, u.school, u.career, u.bio, u.age_group, u.country, u.discord_handle, u.avatar_url,
       t.task_summary
from matches m
join users u on u.id = case when m.user_a = %(me)s then m.user_b else m.user_a end
left join curr_task t on t.user_id = u.id
where %(me)s in (m.user_a, m.user_b)
"""


def _match_view(row: dict) -> dict:
    other = {
        "id": row["other_id"],
        **{key: row[key] for key in ("name", "school", "career", "bio", "age_group", "country",
                                     "discord_handle", "avatar_url", "task_summary")},
    }
    view = {key: row[key] for key in ("match_id", "topic", "score", "created_at",
                                      "my_status", "their_status", "expired",
                                      "my_keep", "their_keep", "connected")}
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



# KEEP IN TOUCH: after both accept, each side answers "keep in touch?". When
# both say yes, create the connection on the match's topic.
@app.post("/users/{user_id}/matches/{match_id}/keep", response_model=MatchView)
def keep_in_touch(user_id: UUID, match_id: int, body: KeepBody) -> dict:
    with connect() as conn:
        updated = conn.execute(
            """
            update matches set
                a_keep = case when user_a = %(me)s then %(keep)s else a_keep end,
                b_keep = case when user_b = %(me)s then %(keep)s else b_keep end
            where id = %(id)s
              and %(me)s in (user_a, user_b)
              and a_status = 'accepted' and b_status = 'accepted'
              and (case when user_a = %(me)s then a_keep else b_keep end) is null
            returning user_a, user_b, topic, a_keep, b_keep
            """,
            {"me": user_id, "keep": body.keep, "id": match_id},
        ).fetchone()
        if updated is not None and updated["a_keep"] and updated["b_keep"]:
            conn.execute(
                """
                insert into connections (user_a, user_b, topic, match_id)
                values (%s, %s, %s, %s)
                on conflict (user_a, user_b, topic) do nothing
                """,
                (updated["user_a"], updated["user_b"], updated["topic"], match_id),
            )
        row = conn.execute(MATCH_VIEW_SQL + " and m.id = %(id)s", {"me": user_id, "id": match_id}).fetchone()
    if row is None:
        raise HTTPException(404, "match not found")
    if updated is None:
        raise HTTPException(409, "you can answer only once, after you both accepted")
    return _match_view(row)


# ACTIVE CONNECTIONS (every ~3 s from the app): which of my connections are
# working on our topic right now, so the app can light them up.
@app.get("/users/{user_id}/connections/active", response_model=list[Connection])
def get_active_connections(user_id: UUID) -> list[dict]:
    with connect() as conn:
        return active_connections(conn, user_id)


# WORK FRIENDS (every ~3 s from the app): all my connections, active first.
@app.get("/users/{user_id}/connections", response_model=list[Connection])
def get_connections(user_id: UUID) -> list[dict]:
    with connect() as conn:
        return list_connections(conn, user_id)


# JOIN A FRIEND: clicking an active friend starts a match with them on the
# connection's topic, with my side already accepted. They get the usual popup.
@app.post("/users/{user_id}/connections/{connection_id}/join", response_model=MatchView)
def join_connection(user_id: UUID, connection_id: int) -> dict:
    with connect() as conn:
        connection = conn.execute(
            "select user_a, user_b, topic from connections where id = %s and %s in (user_a, user_b)",
            (connection_id, user_id),
        ).fetchone()
        if connection is None:
            raise HTTPException(404, "connection not found")
        friend = connection["user_b"] if connection["user_a"] == user_id else connection["user_a"]
        clear_expired_pending(conn, user_id, friend)
        match = conn.execute(
            "select create_match(%s, %s, %s, 1.0::real) as id", (user_id, friend, connection["topic"])
        ).fetchone()
        if match["id"] is None:
            raise HTTPException(409, "one of you already has a match waiting for an answer")
        conn.execute(
            """
            update matches set
                a_status = case when user_a = %(me)s then 'accepted' else a_status end,
                b_status = case when user_b = %(me)s then 'accepted' else b_status end
            where id = %(id)s
            """,
            {"me": user_id, "id": match["id"]},
        )
        row = conn.execute(MATCH_VIEW_SQL + " and m.id = %(id)s", {"me": user_id, "id": match["id"]}).fetchone()
    return _match_view(row)



# TASK HISTORY: my past pushes, newest first (debugging, looking back at work).
@app.get("/users/{user_id}/task-history", response_model=list[TaskHistoryItem])
def task_history(user_id: UUID, limit: int = Query(default=50, ge=1, le=500)) -> list[dict]:
    with connect() as conn:
        return conn.execute(
            """
            select id, task_summary, topic, pushed_at from curr_task_history
            where user_id = %s order by pushed_at desc limit %s
            """,
            (user_id, limit),
        ).fetchall()
