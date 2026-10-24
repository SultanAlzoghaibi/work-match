"""Score Jev matching against hand-labelled pairs (match_pairs.json).

    cd backend/match-eval
    ../.venv/bin/python run_eval.py              # all variants, needs JEV_API
    ../.venv/bin/python run_eval.py --offline    # word-overlap baseline, no key

Variants:
  current     Sultan's production code, run as-is (backend/fastapi-sultan/app/jev.py).
  with-topic  Same question, but each line also carries the summarizer's topic label.
  strict      Same state as current; criteria spell out that the same label on a
              different subject, field or city is not a match.
  topic+strict  Both changes.

Pairs that share the same `a` summary go in one request, like production does
with several candidates, so cross-talk between candidates shows up here too.
JEV_API is read from the environment, then backend/match-eval/.env, then
backend/fastapi-sultan/.env (read only).
"""

import argparse
import importlib.util
import json
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

import httpx

HERE = Path(__file__).resolve().parent
SULTAN = HERE.parent / "fastapi-sultan"


def load_key() -> str | None:
    if os.environ.get("JEV_API"):
        return os.environ["JEV_API"]
    for env in (HERE / ".env", SULTAN / ".env"):
        if env.exists():
            for line in env.read_text().splitlines():
                if line.strip().startswith("JEV_API="):
                    return line.split("=", 1)[1].strip().strip('"')
    return None


def load_sultan_jev():
    spec = importlib.util.spec_from_file_location("sultan_jev", SULTAN / "app" / "jev.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# Variants ----------------------------------------------------------------------

STRICT_CRITERIA = {
    "true": (
        "Same kind of activity on the same subject or goal, so they could help each other: "
        "both practicing coding interview problems, both in the same course or subject, "
        "both building the same kind of software, or both apartment hunting in the same city. "
        "Different specifics are fine (two different LeetCode problems, a lecture and a "
        "homework on the same algorithm)."
    ),
    "false": (
        "Different activities, or the same broad label on a different subject, field or "
        "place: studying biology vs studying computer science, apartments in San Francisco "
        "vs New York, software jobs vs nursing jobs, an essay vs a novel."
    ),
}


def line(prefix: str, side: dict, with_topic: bool) -> str:
    if with_topic and side.get("topic"):
        return f"{prefix} ({side['topic']}): {side['task_summary']}"
    return f"{prefix}: {side['task_summary']}"


def ask_jev(key: str, me: dict, others: list[dict], with_topic: bool, strict: bool, sultan) -> list[float]:
    """Same request shape as production; only the state lines / criteria change."""
    state = [line("Me", me, with_topic)]
    questions = {}
    for index, other in enumerate(others, start=1):
        state.append(line(f"Person {index}", other, with_topic))
        questions[f"c{index}"] = {
            "type": "noul",
            "instructions": (
                f"Are Me and Person {index} doing the same kind of activity, so that "
                "working side by side would help them both?"
            ),
            "criteria": STRICT_CRITERIA if strict else {
                "true": (
                    "Same kind of activity and goal (e.g. both practicing coding interview "
                    "problems, or both searching for an apartment in the same city), even "
                    "with different specifics."
                ),
                "false": "Different kinds of activities or goals.",
            },
        }
    response = httpx.post(
        sultan.JEV_URL,
        headers={"Authorization": f"Bearer {key}"},
        json={"model": sultan.JEV_MODEL, "state": state, "questions": questions},
        timeout=60,
    )
    response.raise_for_status()
    answers = response.json()["answers"]
    return [float(answers[f"c{i}"]["noul"]) for i in range(1, len(others) + 1)]


def score_current(sultan, me: dict, others: list[dict]) -> list[float]:
    """Production code path, unchanged."""
    candidates = [{"user_id": i, "task_summary": o["task_summary"]} for i, o in enumerate(others)]
    scores = sultan.score_candidates(me["task_summary"], candidates)
    return [scores[i] for i in range(len(others))]


WORD = re.compile(r"[a-z0-9]+")
STOP = {"a", "an", "the", "and", "or", "of", "for", "in", "on", "to", "with", "at", "near", "next"}


def score_overlap(me: dict, others: list[dict]) -> list[float]:
    words = lambda s: set(WORD.findall(s.lower())) - STOP
    mine = words(me["topic"] + " " + me["task_summary"])
    out = []
    for o in others:
        theirs = words(o["topic"] + " " + o["task_summary"])
        smaller = min(len(mine), len(theirs)) or 1
        out.append(len(mine & theirs) / smaller)
    return out


# Report ------------------------------------------------------------------------

def evaluate(name: str, pairs: list[dict], scorer, threshold: float) -> dict:
    groups = defaultdict(list)
    for pair in pairs:
        groups[json.dumps(pair["a"], sort_keys=True)].append(pair)
    scored = []
    for group in groups.values():
        scores = scorer(group[0]["a"], [p["b"] for p in group])
        scored += [(p, s) for p, s in zip(group, scores)]

    def accuracy(t):
        return sum((s >= t) == p["match"] for p, s in scored) / len(scored)

    best_t = max((t / 100 for t in range(5, 100, 5)), key=lambda t: (accuracy(t), -abs(t - threshold)))
    yes = [s for p, s in scored if p["match"]]
    no = [s for p, s in scored if not p["match"]]
    wrong = [(p, s) for p, s in scored if (s >= threshold) != p["match"]]
    return {
        "variant": name, "accuracy": accuracy(threshold), "best_threshold": best_t,
        "best_accuracy": accuracy(best_t), "mean_match": sum(yes) / len(yes),
        "mean_nonmatch": sum(no) / len(no), "lowest_match": min(yes), "highest_nonmatch": max(no),
        "wrong": [{"id": p["id"], "kind": p["kind"], "expected": p["match"], "score": round(s, 3)} for p, s in wrong],
        "scores": {p["id"]: round(s, 3) for p, s in scored},
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--offline", action="store_true", help="word-overlap baseline, no API calls")
    parser.add_argument("--variant", action="append", help="current, with-topic, strict, topic+strict")
    parser.add_argument("--threshold", type=float, default=float(os.environ.get("MATCH_THRESHOLD", "0.8")))
    parser.add_argument("--json", help="also write full results here")
    args = parser.parse_args()

    pairs = json.loads((HERE / "match_pairs.json").read_text())["pairs"]
    print(f"{len(pairs)} pairs ({sum(p['match'] for p in pairs)} should match), threshold {args.threshold}\n")

    if args.offline:
        runs = [("word-overlap", score_overlap)]
    else:
        key = load_key()
        if not key:
            sys.exit("JEV_API not found (env, backend/match-eval/.env or backend/fastapi-sultan/.env). Try --offline.")
        os.environ["JEV_API"] = key  # Sultan's score_candidates reads it from the environment
        sultan = load_sultan_jev()
        variants = {
            "current": lambda me, others: score_current(sultan, me, others),
            "with-topic": lambda me, others: ask_jev(key, me, others, True, False, sultan),
            "strict": lambda me, others: ask_jev(key, me, others, False, True, sultan),
            "topic+strict": lambda me, others: ask_jev(key, me, others, True, True, sultan),
        }
        runs = [(v, variants[v]) for v in (args.variant or variants)]

    results = []
    for name, scorer in runs:
        r = evaluate(name, pairs, scorer, args.threshold)
        results.append(r)
        print(f"{name:13} accuracy {r['accuracy']:.0%} at {args.threshold} | best {r['best_accuracy']:.0%} at {r['best_threshold']:.2f} "
              f"| mean score: match {r['mean_match']:.2f}, non-match {r['mean_nonmatch']:.2f} "
              f"| lowest match {r['lowest_match']:.2f}, highest non-match {r['highest_nonmatch']:.2f}")
        for w in r["wrong"]:
            print(f"    wrong: {w['id']:32} {w['kind']:45} expected {'match' if w['expected'] else 'no match':8} score {w['score']:.2f}")
    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=2))
        print(f"\nfull results: {args.json}")


if __name__ == "__main__":
    main()
