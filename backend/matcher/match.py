"""One matching pass: for each eligible user (newest summary first), score
their candidates, take the best one above both thresholds, write intros and
create the offer. A user gets at most one offer per pass."""

from dataclasses import dataclass
from typing import Protocol

from .intro import Intro
from .jev import Score
from .people import Person


class Store(Protocol):
    def eligible_users(self) -> list[Person]: ...
    def candidates(self, user_id: str) -> list[Person]: ...
    def recent_outcomes(self) -> list[dict]: ...
    def create_offer(self, **kwargs) -> int | None: ...


class Scorer(Protocol):
    def score_many(self, me: Person, others: list[Person], outcomes: list[dict]) -> list[Score]: ...


class IntroWriter(Protocol):
    def write(self, me: Person, other: Person) -> Intro: ...


@dataclass(frozen=True)
class Thresholds:
    match: float = 0.7
    interrupt: float = 0.5


def best_candidate(scores: list[Score], thresholds: Thresholds) -> Score | None:
    passing = [s for s in scores if s.match >= thresholds.match and s.interrupt_ok >= thresholds.interrupt]
    return max(passing, key=lambda s: s.match, default=None)


def run_once(store: Store, scorer: Scorer, writer: IntroWriter, thresholds: Thresholds) -> list[int]:
    users = store.eligible_users()
    if len(users) < 2:
        return []
    outcomes = store.recent_outcomes()
    taken: set[str] = set()
    created: list[int] = []

    for me in users:
        if me.user_id in taken:
            continue
        candidates = [c for c in store.candidates(me.user_id) if c.user_id not in taken]
        if not candidates:
            continue
        best = best_candidate(scorer.score_many(me, candidates, outcomes), thresholds)
        if best is None:
            continue

        other = best.candidate
        intro = writer.write(me, other)
        offer_id = store.create_offer(
            one=me, two=other, topic=intro.topic, score=best.match,
            intro_one=intro.intro_a, intro_two=intro.intro_b,
            interrupt_ok=best.interrupt_ok,
            decision_model=best.model, intro_model=intro.model,
        )
        if offer_id is not None:
            taken.update((me.user_id, other.user_id))
            created.append(offer_id)
            print(f"[match] offer {offer_id}: {me.name} + {other.name} on {intro.topic!r} "
                  f"(match {best.match:.2f}, interrupt {best.interrupt_ok:.2f}, {best.model})")
    return created
