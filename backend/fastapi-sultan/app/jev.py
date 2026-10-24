"""Jev (TypeSafe) client: scores how well my task matches each candidate's.

Same request shape as test_jev.py. One HTTP call per push: every candidate is
its own yes/no ("noul") question over a shared state.
"""

import os

import httpx

JEV_URL = "https://api.typesafe.ai/v1/systemone"
JEV_MODEL = "jev-latest"
MAX_CANDIDATES = 20


class JevError(Exception):
    pass


def _line(label: str, topic: str | None, summary: str) -> str:
    return f"{label} ({topic}): {summary}" if topic else f"{label}: {summary}"


def score_candidates(my_summary: str, candidates: list[dict], my_topic: str | None = None) -> dict:
    """Return {candidate user_id: P(good match)} for up to MAX_CANDIDATES.

    Candidates are labelled "Person 1", "Person 2", ... (no names), so Jev
    judges the work, not who is doing it. Each line carries the summarizer's
    topic label when there is one.
    """
    key = os.environ.get("JEV_API")
    if not key:
        raise JevError("JEV_API is not set in backend/fastapi-sultan/.env")
    candidates = candidates[:MAX_CANDIDATES]
    if not candidates:
        return {}

    state = [_line("Me", my_topic, my_summary)]
    questions = {}
    for index, candidate in enumerate(candidates, start=1):
        state.append(_line(f"Person {index}", candidate.get("topic"), candidate["task_summary"]))
        questions[f"c{index}"] = {
            "type": "noul",
            # Criteria, topic labels and MATCH_THRESHOLD=0.45 were measured
            # together in backend/match-eval ("general-agnostic"): 96% of 105
            # labelled pairs right, 100% on different city / event / company.
            # Change them together and re-run the eval.
            "instructions": (
                f"Are Me and Person {index} doing the same kind of activity, so that "
                "working side by side would help them both?"
            ),
            "criteria": {
                "true": (
                    "They are doing the same kind of activity toward the same subject or "
                    "goal, so they could help or motivate each other right now. The details "
                    "can differ: different problems, materials, tools or stages of the same "
                    "subject still count, and so does one person learning the subject while "
                    "the other practices or applies it."
                ),
                "false": (
                    "Their activities are different, or they only share a broad category of "
                    "activity while the specific subject, field or goal is different. Being in "
                    "a different city, at a different event or at a different company doesn't "
                    "matter."
                ),
            },
        }

    try:
        response = httpx.post(
            JEV_URL,
            headers={"Authorization": f"Bearer {key}"},
            json={"model": JEV_MODEL, "state": state, "questions": questions},
            timeout=30,
        )
        response.raise_for_status()
        answers = response.json()["answers"]
    except (httpx.HTTPError, KeyError, ValueError) as err:
        raise JevError(f"Jev request failed: {err}") from err

    return {
        candidate["user_id"]: float(answers[f"c{index}"]["noul"])
        for index, candidate in enumerate(candidates, start=1)
        if f"c{index}" in answers
    }


def score_topics(my_summary: str, topics: list[str]) -> list[float]:
    """For each connection topic, P(my current task is about that topic).

    One request; used on push to light up connections (is_working_on_connections).
    """
    key = os.environ.get("JEV_API")
    if not key:
        raise JevError("JEV_API is not set in backend/fastapi-sultan/.env")
    if not topics:
        return []

    questions = {
        f"t{index}": {
            "type": "noul",
            "instructions": f'Is Me currently working on the topic "{topic}"?',
            "criteria": {
                "true": f'The task is about "{topic}" or a closely related activity, even if worded differently.',
                "false": f'The task is about something other than "{topic}".',
            },
        }
        for index, topic in enumerate(topics)
    }
    try:
        response = httpx.post(
            JEV_URL,
            headers={"Authorization": f"Bearer {key}"},
            json={"model": JEV_MODEL, "state": [f"Me: {my_summary}"], "questions": questions},
            timeout=30,
        )
        response.raise_for_status()
        answers = response.json()["answers"]
    except (httpx.HTTPError, KeyError, ValueError) as err:
        raise JevError(f"Jev request failed: {err}") from err
    return [float(answers.get(f"t{index}", {}).get("noul", 0.0)) for index in range(len(topics))]
