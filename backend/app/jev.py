"""Match scoring with Jev (TypeSafe's decision model, via OpenRouter).

Each pair gets one request with three yes/no ("noul") questions: is this a
good match, and is each person open to an intro right now. Without an API key
we fall back to word overlap between topics so the rest of the app still works.
"""

import re
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass

import httpx

from .people import Person

DECISIONS_URL = "https://openrouter.ai/api/alpha/decisions"

FRAMING = (
    "We are deciding whether to introduce two people who are each working alone "
    "right now, so they can work alongside each other in a 25-minute co-working session."
)

QUESTIONS = {
    "good_match": {
        "type": "noul",
        "instructions": (
            "Person A and Person B are working on similar enough things right now "
            "that working alongside each other would help both of them."
        ),
        "criteria": {
            "true": "Their current work clearly overlaps in topic or goal, e.g. the same course, algorithm, tool or kind of project.",
            "false": "Their work is unrelated, or only similar in a superficial way such as both using a browser.",
        },
    },
    "a_open": {
        "type": "noul",
        "instructions": "Person A would welcome a short, friendly intro to someone right now.",
        "criteria": {
            "true": "An intro would not hurt their work. Being stuck makes an intro more welcome.",
            "false": "They are in deep focus on something where an interruption would be unwelcome.",
        },
    },
    "b_open": {
        "type": "noul",
        "instructions": "Person B would welcome a short, friendly intro to someone right now.",
        "criteria": {
            "true": "An intro would not hurt their work. Being stuck makes an intro more welcome.",
            "false": "They are in deep focus on something where an interruption would be unwelcome.",
        },
    },
}


@dataclass(frozen=True)
class Score:
    candidate: Person
    match: float          # P(good match)
    interrupt_ok: float   # min of both people's P(open to an intro)
    model: str


def _outcome_lines(outcomes: list[dict]) -> list[str]:
    lines = []
    for o in outcomes:
        accepted = o["a_status"] == "accepted" and o["b_status"] == "accepted"
        if o.get("connection_status") == "friends":
            result = "both accepted and became friends"
        elif accepted:
            result = "both accepted"
        else:
            result = "not accepted"
        lines.append(f"Past intro: A: {o['summary_a']} | B: {o['summary_b']} | Outcome: {result}.")
    return lines


def _noul(answers: dict, key: str) -> float:
    value = answers[key]["noul"]
    return max(0.0, min(1.0, float(value)))


class JevScorer:
    def __init__(self, api_key: str, model: str, timeout: float = 20.0):
        self.model = model
        self._client = httpx.Client(
            timeout=timeout,
            headers={"Authorization": f"Bearer {api_key}"},
        )

    def score_pair(self, me: Person, other: Person, outcomes: list[dict]) -> Score:
        body = {
            "model": self.model,
            "state": [
                FRAMING,
                f"Person A: {me.describe()}",
                f"Person B: {other.describe()}",
                *_outcome_lines(outcomes),
            ],
            "questions": QUESTIONS,
        }
        resp = self._client.post(DECISIONS_URL, json=body)
        resp.raise_for_status()
        data = resp.json()
        answers = data.get("answers", data)
        return Score(
            candidate=other,
            match=_noul(answers, "good_match"),
            interrupt_ok=min(_noul(answers, "a_open"), _noul(answers, "b_open")),
            model=self.model,
        )

    def score_many(self, me: Person, others: list[Person], outcomes: list[dict]) -> list[Score]:
        with ThreadPoolExecutor(max_workers=8) as pool:
            futures = [pool.submit(self.score_pair, me, o, outcomes) for o in others]
            results = []
            for f in futures:
                try:
                    results.append(f.result())
                except (httpx.HTTPError, KeyError, ValueError, TypeError) as err:
                    print(f"[jev] scoring failed: {err}")
            return results


_WORD = re.compile(r"[a-z0-9+#]+")
_STOPWORDS = {"a", "an", "the", "and", "or", "of", "for", "in", "on", "to", "with"}


def _words(p: Person) -> set[str]:
    text = " ".join([p.topic, *p.subtopics, p.context or ""]).lower()
    return set(_WORD.findall(text)) - _STOPWORDS


def _open(p: Person) -> float:
    return 0.3 if p.focus == "high" and not p.stuck else 0.8


class FallbackScorer:
    """Topic word overlap, used when no Jev key is configured. The same topic
    counts as a strong match; otherwise shared words over the smaller set."""

    model = "fallback:topic"

    def score_many(self, me: Person, others: list[Person], outcomes: list[dict]) -> list[Score]:
        mine = _words(me)
        scores = []
        for o in others:
            theirs = _words(o)
            smaller = min(len(mine), len(theirs))
            overlap = len(mine & theirs) / smaller if smaller else 0.0
            if me.topic.strip().lower() == o.topic.strip().lower():
                overlap = max(overlap, 0.8)
            scores.append(Score(o, overlap, min(_open(me), _open(o)), self.model))
        return scores
