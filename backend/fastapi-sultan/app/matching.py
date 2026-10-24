"""Match search that runs after each push (JEV SEARCH in backend/schema.sql).

prefilter candidates -> Jev scores -> create_match() for the best one at or
above MATCH_THRESHOLD.
"""

import os
from uuid import UUID

from . import jev


def _threshold() -> float:
    return float(os.environ.get("MATCH_THRESHOLD", "0.8"))


def _decline_cooldown_minutes() -> int:
    # Default 1 day, per docs/DATABASE.md. Set to 0 in .env while testing.
    return int(os.environ.get("DECLINE_COOLDOWN_MINUTES", "1440"))


CANDIDATES_SQL = """
select t.user_id, t.task_summary, u.name
from curr_task t
join users u on u.id = t.user_id
where t.user_id <> %(me)s
  and u.sharing_enabled
  and t.pushed_at > now() - interval '11 minutes'                     -- skip dead laptops
  and coalesce(u.age_group = 'under_18', false) = %(me_minor)s        -- minors only with minors
  and (not %(same_country)s or u.country = %(my_country)s)
  and (not %(same_age_group)s or u.age_group = %(my_age_group)s)
  and not exists (                                                    -- already connected on topic
      select 1 from connections c
      where c.user_a = least(%(me)s, t.user_id) and c.user_b = greatest(%(me)s, t.user_id)
        and c.topic = %(topic)s)
  and not exists (                                                    -- declined recently
      select 1 from matches m
      where m.user_a = least(%(me)s, t.user_id) and m.user_b = greatest(%(me)s, t.user_id)
        and 'declined' in (m.a_status, m.b_status)
        and m.created_at > now() - make_interval(mins => %(cooldown)s))
order by t.pushed_at desc
limit %(limit)s
"""


def find_match(conn, me: UUID, my_summary: str, topic: str, filters) -> dict:
    """Run the search for `me` right after their push. Returns a MatchResult dict."""
    result = {"match_id": None, "other_name": None, "scores": [], "skipped_reason": None}

    profile = conn.execute(
        "select sharing_enabled, age_group, country from users where id = %s", (me,)
    ).fetchone()
    if profile is None or not profile["sharing_enabled"]:
        result["skipped_reason"] = "sharing is off"
        return result

    candidates = conn.execute(
        CANDIDATES_SQL,
        {
            "me": me,
            "me_minor": profile["age_group"] == "under_18",
            "same_country": filters.same_country,
            "my_country": profile["country"],
            "same_age_group": filters.same_age_group,
            "my_age_group": profile["age_group"],
            "topic": topic,
            "cooldown": _decline_cooldown_minutes(),
            "limit": jev.MAX_CANDIDATES,
        },
    ).fetchall()
    if not candidates:
        result["skipped_reason"] = "no one else matching your filters is working right now"
        return result

    try:
        scores = jev.score_candidates(my_summary, candidates)
    except jev.JevError as err:
        result["skipped_reason"] = str(err)
        return result

    names = {candidate["user_id"]: candidate["name"] for candidate in candidates}
    ranked = sorted(scores.items(), key=lambda item: item[1], reverse=True)
    result["scores"] = [
        {"user_id": user_id, "name": names[user_id], "score": round(score, 3)}
        for user_id, score in ranked
    ]

    # create_match() returns null if either person already has a pending popup
    # or a concurrent push made the same pair, so try the next best.
    for user_id, score in ranked:
        if score < _threshold():
            break
        row = conn.execute(
            "select create_match(%s, %s, %s, %s::real) as id", (me, user_id, topic, score)
        ).fetchone()
        conn.commit()
        if row["id"] is not None:
            result["match_id"] = row["id"]
            result["other_name"] = names[user_id]
            break
    return result
