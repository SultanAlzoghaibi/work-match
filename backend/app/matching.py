"""Jev search for one user, run after each push (docs/DATABASE.md, "Timing")."""

import psycopg

from .config import Config
from .jev import FallbackScorer, JevScorer, Score
from .people import Person

# Prefilter from backend/schema.sql ("JEV SEARCH"): working now, sharing on,
# compatible age group, not connected on this topic, not declined in the last day.
CANDIDATES_SQL = """
select t.user_id, t.task_summary, t.topic, u.name, u.school, u.bio
from curr_task t join users u on u.id = t.user_id
where t.user_id <> %(me)s
  and u.sharing_enabled
  and t.pushed_at > now() - interval '11 minutes'
  and (u.age_group = 'under_18') is not distinct from (%(my_age)s = 'under_18')
  and not exists (select 1 from connections c
                  where c.user_a = least(%(me)s, t.user_id) and c.user_b = greatest(%(me)s, t.user_id)
                    and c.topic = t.topic)
  and not exists (select 1 from matches m
                  where m.user_a = least(%(me)s, t.user_id) and m.user_b = greatest(%(me)s, t.user_id)
                    and 'declined' in (m.a_status, m.b_status)
                    and m.created_at > now() - interval '1 day')
limit 10
"""

ME_SQL = """
select t.user_id, t.task_summary, t.topic, u.name, u.school, u.bio, u.age_group, u.sharing_enabled
from curr_task t join users u on u.id = t.user_id
where t.user_id = %(me)s
"""


def make_scorer(cfg: Config):
    return JevScorer(cfg.openrouter_api_key, cfg.jev_model) if cfg.openrouter_api_key else FallbackScorer()


def best(scores: list[Score], cfg: Config) -> Score | None:
    passing = [s for s in scores if s.match >= cfg.match_threshold and s.interrupt_ok >= cfg.interrupt_threshold]
    return max(passing, key=lambda s: s.match, default=None)


def search(conn: psycopg.Connection, me_id: int, cfg: Config, scorer) -> int | None:
    """Score this user against everyone eligible; create at most one match.
    Returns the new match id, or None."""
    me_row = conn.execute(ME_SQL, {"me": me_id}).fetchone()
    if me_row is None or not me_row["sharing_enabled"]:
        return None
    rows = conn.execute(CANDIDATES_SQL, {"me": me_id, "my_age": me_row["age_group"]}).fetchall()
    if not rows:
        return None

    me = Person.from_task(me_row)
    pick = best(scorer.score_many(me, [Person.from_task(r) for r in rows], []), cfg)
    if pick is None:
        return None

    topic = me.topic or pick.candidate.topic or "the same thing"
    row = conn.execute(
        "select create_match(%s::bigint, %s::bigint, %s::text, %s::real) as id",
        (me_id, int(pick.candidate.user_id), topic, pick.match),
    ).fetchone()
    if row["id"] is not None:
        print(f"[match] {row['id']}: {me.name} + {pick.candidate.name} on {topic!r} "
              f"(match {pick.match:.2f}, {pick.model})")
    return row["id"]
