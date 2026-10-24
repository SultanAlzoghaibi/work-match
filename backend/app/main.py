"""work-match API. The Mac app's only way in: push every 5 min, poll every
20 s, answer popups, quit. The database itself is closed to the app
(RLS on, no policies); this server connects with the database owner role.

Auth: POST /users returns a token once. Send it as `Authorization: Bearer <token>`.
Only its SHA-256 hash is stored in users.token.
"""

import hashlib
import secrets
from contextlib import asynccontextmanager
from typing import Literal

from fastapi import BackgroundTasks, Depends, FastAPI, Header, HTTPException
from pydantic import BaseModel, Field

from . import db, matching
from .config import Config

cfg = Config.from_env()
scorer = matching.make_scorer(cfg)

AgeGroup = Literal["under_18", "18_24", "25_34", "35_44", "45_plus"]


@asynccontextmanager
async def lifespan(app: FastAPI):
    if not cfg.database_url:
        raise RuntimeError("Set DATABASE_URL (see backend/.env.example)")
    db.open_pool(cfg.database_url)
    print(f"[api] scorer={scorer.model} thresholds=match {cfg.match_threshold}, "
          f"interrupt {cfg.interrupt_threshold}")
    yield
    db.close_pool()


app = FastAPI(title="work-match API", lifespan=lifespan)


def hash_token(token: str) -> str:
    return hashlib.sha256(token.encode()).hexdigest()


def current_user(authorization: str = Header(default="")) -> int:
    token = authorization.removeprefix("Bearer ").strip()
    if not token:
        raise HTTPException(401, "missing token")
    with db.connection() as conn:
        row = conn.execute("select id from users where token = %s", (hash_token(token),)).fetchone()
    if row is None:
        raise HTTPException(401, "invalid token")
    return row["id"]


# Models ----------------------------------------------------------------------

class NewUser(BaseModel):
    name: str = Field(min_length=1, max_length=80)
    school: str | None = Field(default=None, max_length=120)
    bio: str | None = Field(default=None, max_length=200)
    age_group: AgeGroup | None = None
    country: str | None = Field(default=None, min_length=2, max_length=2)
    sharing_enabled: bool = False


class ProfileUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=80)
    school: str | None = Field(default=None, max_length=120)
    bio: str | None = Field(default=None, max_length=200)
    age_group: AgeGroup | None = None
    country: str | None = Field(default=None, min_length=2, max_length=2)
    sharing_enabled: bool | None = None


class Push(BaseModel):
    task_summary: str = Field(min_length=1, max_length=1000)
    topic: str | None = Field(default=None, max_length=80)


class Answer(BaseModel):
    accept: bool


# Routes ----------------------------------------------------------------------

@app.get("/health")
def health():
    with db.connection() as conn:
        conn.execute("select 1")
    return {"ok": True, "scorer": scorer.model}


@app.post("/users", status_code=201)
def create_user(body: NewUser):
    """Sign up. The token is only ever returned here; the app must store it."""
    token = secrets.token_urlsafe(32)
    with db.connection() as conn:
        row = conn.execute(
            """insert into users (token, name, school, bio, age_group, country, sharing_enabled)
               values (%s, %s, %s, %s, %s, %s, %s) returning id""",
            (hash_token(token), body.name, body.school, body.bio, body.age_group,
             body.country.upper() if body.country else None, body.sharing_enabled),
        ).fetchone()
    return {"id": row["id"], "token": token}


@app.get("/me")
def get_me(me: int = Depends(current_user)):
    with db.connection() as conn:
        return conn.execute(
            "select id, name, school, bio, age_group, country, sharing_enabled, created_at from users where id = %s",
            (me,),
        ).fetchone()


@app.patch("/me")
def update_me(body: ProfileUpdate, me: int = Depends(current_user)):
    fields = body.model_dump(exclude_unset=True)
    if "country" in fields and fields["country"]:
        fields["country"] = fields["country"].upper()
    if fields:
        sets = ", ".join(f"{k} = %({k})s" for k in fields)  # keys come from the model, not the client
        with db.connection() as conn:
            conn.execute(f"update users set {sets} where id = %(me)s", {**fields, "me": me})
    return get_me(me)


def run_search(me: int) -> None:
    try:
        with db.connection() as conn:
            matching.search(conn, me, cfg, scorer)
    except Exception as err:  # a failed search must not break pushes
        print(f"[match] search for {me} failed: {err!r}")


@app.post("/push")
def push(body: Push, background: BackgroundTasks, me: int = Depends(current_user)):
    """Every 5 min: replace my current task, refresh my 'active' rows, then run
    the Jev search for me in the background."""
    with db.connection() as conn:
        conn.execute(
            """insert into curr_task (user_id, task_summary, topic) values (%s, %s, %s)
               on conflict (user_id) do update set
                   task_summary = excluded.task_summary, topic = excluded.topic, pushed_at = now()""",
            (me, body.task_summary, body.topic),
        )
        conn.execute("delete from is_working_on_connections where user_id = %s", (me,))
        conn.execute(
            """insert into is_working_on_connections (connection_id, user_id)
               select id, %(me)s from connections where %(me)s in (user_a, user_b) and topic = %(topic)s""",
            {"me": me, "topic": body.topic},
        )
    background.add_task(run_search, me)
    return {"ok": True}


@app.get("/poll")
def poll(me: int = Depends(current_user)):
    """Every 20 s (every ~2 s while a popup is open)."""
    with db.connection() as conn:
        conn.execute("delete from is_working_on_connections where seen_at < now() - interval '11 minutes'")
        matches = conn.execute(
            """select m.id, m.topic, m.score, m.created_at,
                      case when m.user_a = %(me)s then m.a_status else m.b_status end as my_status,
                      case when m.user_a = %(me)s then m.b_status else m.a_status end as their_status,
                      u.name as other_name, u.school, u.bio, t.task_summary
               from matches m
               join users u on u.id = case when m.user_a = %(me)s then m.user_b else m.user_a end
               left join curr_task t on t.user_id = u.id
               where %(me)s in (m.user_a, m.user_b)
                 and m.created_at > now() - interval '2 minutes'
                 and 'declined' not in (m.a_status, m.b_status)
               order by m.created_at desc""",
            {"me": me},
        ).fetchall()
        active = conn.execute(
            """select c.id as connection_id, c.topic, u.name as friend_name
               from is_working_on_connections w
               join connections c on c.id = w.connection_id
               join users u on u.id = w.user_id
               where %(me)s in (c.user_a, c.user_b) and w.user_id <> %(me)s""",
            {"me": me},
        ).fetchall()
    for m in matches:
        m["start_call"] = m["my_status"] == "accepted" and m["their_status"] == "accepted"
    return {"matches": matches, "active_connections": active}


@app.post("/matches/{match_id}/respond")
def respond(match_id: int, body: Answer, me: int = Depends(current_user)):
    """Work together / Not now. Only your own side, only within 2 minutes."""
    status = "accepted" if body.accept else "declined"
    with db.connection() as conn:
        row = conn.execute(
            """update matches set
                   a_status = case when user_a = %(me)s then %(s)s else a_status end,
                   b_status = case when user_b = %(me)s then %(s)s else b_status end
               where id = %(id)s and %(me)s in (user_a, user_b)
                 and created_at > now() - interval '2 minutes'
                 and case when user_a = %(me)s then a_status else b_status end = 'pending'
               returning a_status, b_status""",
            {"me": me, "s": status, "id": match_id},
        ).fetchone()
    if row is None:
        raise HTTPException(409, "match not found, expired, or already answered")
    both = row["a_status"] == "accepted" and row["b_status"] == "accepted"
    return {"a_status": row["a_status"], "b_status": row["b_status"], "start_call": both}


@app.post("/quit")
def quit_(me: int = Depends(current_user)):
    """Quit or pause: nobody gets matched with, or pinged about, me."""
    with db.connection() as conn:
        conn.execute("delete from curr_task where user_id = %s", (me,))
        conn.execute("delete from is_working_on_connections where user_id = %s", (me,))
    return {"ok": True}
