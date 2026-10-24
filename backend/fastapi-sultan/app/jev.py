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


def score_candidates(my_summary: str, candidates: list[dict]) -> dict:
    """Return {candidate user_id: P(good match)} for up to MAX_CANDIDATES.

    Candidates are labelled "Person 1", "Person 2", ... (no names), so Jev
    judges the work, not who is doing it.
    """
    key = os.environ.get("JEV_API")
    if not key:
        raise JevError("JEV_API is not set in backend/fastapi-sultan/.env")
    candidates = candidates[:MAX_CANDIDATES]
    if not candidates:
        return {}

    state = [f"Me: {my_summary}"]
    questions = {}
    for index, candidate in enumerate(candidates, start=1):
        state.append(f"Person {index}: {candidate['task_summary']}")
        questions[f"c{index}"] = {
            "type": "noul",
            # Wording picked by comparing sample pairs: same-activity pairs
            # score ~0.9, unrelated ones ~0.05 (a stricter "same task" wording
            # put two LeetCode sessions at ~0.5).
            "instructions": (
                f"Are Me and Person {index} doing the same kind of activity, so that "
                "working side by side would help them both?"
            ),
            "criteria": {
                "true": (
                    "Same kind of activity and goal (e.g. both practicing coding interview "
                    "problems, or both searching for an apartment in the same city), even "
                    "with different specifics."
                ),
                "false": "Different kinds of activities or goals.",
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
