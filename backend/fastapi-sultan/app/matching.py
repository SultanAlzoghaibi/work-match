"""Match search that runs after each push (JEV SEARCH in backend/schema.sql).

prefilter candidates -> Jev scores -> create_match() for the best one at or
above MATCH_THRESHOLD.
"""

import os
from uuid import UUID

from . import jev


def _threshold() -> float:
    # Measured in backend/match-eval together with the criteria in jev.py.
    return float(os.environ.get("MATCH_THRESHOLD", "0.45"))


def _active_threshold() -> float:
    # Friend lights (score_topics) ask a different question that match-eval
    # doesn't cover, so they keep the old cutoff.
    return float(os.environ.get("ACTIVE_TOPIC_THRESHOLD", "0.8"))


def _match_cooldown_minutes() -> int:
    """How long a pair isn't matched again after any match between them.

    Covers declined pairs (1 day per docs/DATABASE.md) and also pairs that just
    matched, since the app re-pushes every few seconds during a session. Use a
    short value like 1 in .env while testing. DECLINE_COOLDOWN_MINUTES is the
    old name.
    """
    value = os.environ.get("MATCH_COOLDOWN_MINUTES") or os.environ.get("DECLINE_COOLDOWN_MINUTES")
    return int(value or "1440")


CANDIDATES_SQL = """
select t.user_id, t.task_summary, t.topic, u.name
from curr_task t
join users u on u.id = t.user_id
join users me on me.id = %(me)s
left join user_filters mf on mf.user_id = me.id                       -- my filters
left join user_filters cf on cf.user_id = u.id                        -- their filters
where t.user_id <> %(me)s
  and u.sharing_enabled
  and t.pushed_at > now() - interval '11 minutes'                     -- skip dead laptops
  and coalesce(u.age_group = 'under_18', false) = %(me_minor)s        -- minors only with minors
  and (not %(same_country)s or u.country = %(my_country)s)
  and (not %(same_age_group)s or u.age_group = %(my_age_group)s)
  -- Saved filters (Filters tab), mutual: both people's filters must allow the pair.
  -- Empty list = any. A filter on age/country excludes people who haven't set theirs.
  and (cardinality(coalesce(mf.age_groups, '{}')) = 0 or u.age_group = any(mf.age_groups))
  and (cardinality(coalesce(cf.age_groups, '{}')) = 0 or me.age_group = any(cf.age_groups))
  and (cardinality(coalesce(mf.countries, '{}')) = 0 or u.country = any(mf.countries))
  and (cardinality(coalesce(cf.countries, '{}')) = 0 or me.country = any(cf.countries))
  and (not (coalesce(mf.shared_language, false) or coalesce(cf.shared_language, false))
       or u.languages && me.languages)                                -- share at least one language
  and not exists (                                                    -- already connected on topic
      select 1 from connections c
      where c.user_a = least(%(me)s, t.user_id) and c.user_b = greatest(%(me)s, t.user_id)
        and c.topic = %(topic)s)
  and not exists (                                                    -- matched recently
      select 1 from matches m
      where m.user_a = least(%(me)s, t.user_id) and m.user_b = greatest(%(me)s, t.user_id)
        and m.created_at > now() - make_interval(mins => %(cooldown)s))
order by t.pushed_at desc
limit %(limit)s
"""


def clear_expired_pending(conn, user_1: UUID, user_2: UUID) -> None:
    """Drop this pair's popups that expired with nobody answering.

    uq_matches_pending_pair allows one both-pending match per pair and can't
    know about the 2-minute expiry, so an ignored popup would block the pair
    forever.
    """
    conn.execute(
        """
        delete from matches
        where user_a = least(%(a)s, %(b)s) and user_b = greatest(%(a)s, %(b)s)
          and a_status = 'pending' and b_status = 'pending'
          and created_at < now() - interval '2 minutes'
        """,
        {"a": user_1, "b": user_2},
    )


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
            "cooldown": _match_cooldown_minutes(),
            "limit": jev.MAX_CANDIDATES,
        },
    ).fetchall()
    if not candidates:
        result["skipped_reason"] = (
            "no one available: nobody else is sharing a task that fits your filters, "
            "or you were matched with them recently"
        )
        return result

    try:
        scores = jev.score_candidates(my_summary, candidates, my_topic=topic)
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
        clear_expired_pending(conn, me, user_id)
        row = conn.execute(
            "select create_match(%s, %s, %s, %s::real) as id", (me, user_id, topic, score)
        ).fetchone()
        conn.commit()
        if row["id"] is not None:
            result["match_id"] = row["id"]
            result["other_name"] = names[user_id]
            break
    return result


def _active_seconds() -> int:
    return int(os.environ.get("ACTIVE_SECONDS", "660"))


def refresh_active_connections(conn, me: UUID, my_summary: str) -> list[dict]:
    """Light up my connections whose topic matches what I'm doing now.

    Rewrites my is_working_on_connections rows: a row per connection whose
    topic Jev scores at or above ACTIVE_TOPIC_THRESHOLD, nothing for the rest (so
    switching tasks turns the old light off). Returns [{topic, score}].
    """
    connections = conn.execute(
        "select id, topic from connections where %s in (user_a, user_b)", (me,)
    ).fetchall()
    conn.execute("delete from is_working_on_connections where user_id = %s", (me,))
    if not connections:
        return []

    # Score each distinct topic once (several friends can share a topic).
    topics = sorted({row["topic"] for row in connections})
    try:
        scores = dict(zip(topics, jev.score_topics(my_summary, topics)))
    except jev.JevError as err:
        print(f"[active] {err}")
        return []

    for row in connections:
        if scores[row["topic"]] >= _active_threshold():
            conn.execute(
                "insert into is_working_on_connections (connection_id, user_id) values (%s, %s)",
                (row["id"], me),
            )
    return [{"topic": topic, "score": round(score, 3)} for topic, score in scores.items()]


FRIEND_COLUMNS = ("name", "school", "career", "bio", "age_group", "country",
                  "discord_handle", "avatar_url", "languages", "task_summary")

CONNECTIONS_SQL = """
select c.id as connection_id, c.topic, c.created_at,
       w.seen_at,
       u.id as friend_id, u.name, u.school, u.career, u.bio, u.age_group, u.country,
       u.discord_handle, u.avatar_url, u.languages, t.task_summary
from connections c
join users u on u.id = case when c.user_a = %(me)s then c.user_b else c.user_a end
left join is_working_on_connections w on w.connection_id = c.id and w.user_id = u.id
left join curr_task t on t.user_id = u.id
where %(me)s in (c.user_a, c.user_b)
order by (w.seen_at is null), w.seen_at desc, u.name
"""


def _drop_stale_active_rows(conn) -> None:
    # Laptops that stopped pushing (crash, sleep) drop off after ACTIVE_SECONDS.
    conn.execute(
        "delete from is_working_on_connections where seen_at < now() - make_interval(secs => %s)",
        (_active_seconds(),),
    )


def _connection_view(row: dict) -> dict:
    return {
        "connection_id": row["connection_id"],
        "topic": row["topic"],
        "active": row["seen_at"] is not None,
        "seen_at": row["seen_at"],
        "friend": {"id": row["friend_id"], **{key: row[key] for key in FRIEND_COLUMNS}},
    }


def list_connections(conn, me: UUID) -> list[dict]:
    """All my connections; the ones working on our topic right now come first."""
    _drop_stale_active_rows(conn)
    return [_connection_view(row) for row in conn.execute(CONNECTIONS_SQL, {"me": me}).fetchall()]


def active_connections(conn, me: UUID) -> list[dict]:
    """My connections whose other person is working on our topic right now."""
    return [view for view in list_connections(conn, me) if view["active"]]
