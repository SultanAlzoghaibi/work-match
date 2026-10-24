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
  general     Topic + criteria written as general rules, with no examples taken
              from the pair files (the fair version of topic+strict).

Jev answers are cached in .jev_cache.json, keyed by the exact request, so a
re-run only calls Jev for requests it hasn't seen. --no-calls refuses to call
Jev at all (anything not cached is an error).

Pairs that share the same `a` summary go in one request, like production does
with several candidates, so cross-talk between candidates shows up here too.
JEV_API is read from the environment, then backend/match-eval/.env, then
backend/fastapi-sultan/.env (read only).
"""

import argparse
import hashlib
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
    module.httpx = CACHED_HTTPX  # in memory only: caches his requests too
    return module


# Cache ---------------------------------------------------------------------------

CACHE_FILE = HERE / ".jev_cache.json"


class CachedResponse:
    def __init__(self, data: dict):
        self._data = data

    def raise_for_status(self) -> None:
        pass

    def json(self) -> dict:
        return self._data


class CachedHttpx:
    """Stands in for httpx in this script and, in memory only, in Sultan's jev
    module, so every Jev request is answered from .jev_cache.json when possible."""

    HTTPError = httpx.HTTPError

    def __init__(self):
        self.cache = json.loads(CACHE_FILE.read_text()) if CACHE_FILE.exists() else {}
        self.allow_calls = True
        self.calls = 0

    def post(self, url, *, headers, json, timeout):
        import json as _json
        key = hashlib.sha256(_json.dumps([url, json], sort_keys=True).encode()).hexdigest()
        if key not in self.cache:
            if not self.allow_calls:
                raise SystemExit("--no-calls: this request isn't cached yet, so Jev would be called")
            response = httpx.post(url, headers=headers, json=json, timeout=timeout)
            response.raise_for_status()
            self.cache[key] = response.json()
            self.calls += 1
            CACHE_FILE.write_text(_json.dumps(self.cache))
        return CachedResponse(self.cache[key])


CACHED_HTTPX = CachedHttpx()


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


# Same intent as STRICT_CRITERIA, but only general rules: no subjects, places
# or jobs that appear in match_pairs.json or holdout_pairs.json.
GENERAL_CRITERIA = {
    "true": (
        "They are doing the same kind of activity toward the same subject or goal, so "
        "they could help or motivate each other right now. The details can differ: "
        "different problems, materials, tools or stages of the same subject still count, "
        "and so does one person learning the subject while the other practices or applies it."
    ),
    "false": (
        "Their activities are different, or they only share a broad category of activity "
        "while the specific subject, field, goal or place is different."
    ),
}


def line(prefix: str, side: dict, with_topic: bool) -> str:
    if with_topic and side.get("topic"):
        return f"{prefix} ({side['topic']}): {side['task_summary']}"
    return f"{prefix}: {side['task_summary']}"


def ask_jev(key: str, me: dict, others: list[dict], with_topic: bool, criteria: dict | None, sultan) -> list[float]:
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
            "criteria": criteria or {
                "true": (
                    "Same kind of activity and goal (e.g. both practicing coding interview "
                    "problems, or both searching for an apartment in the same city), even "
                    "with different specifics."
                ),
                "false": "Different kinds of activities or goals.",
            },
        }
    response = CACHED_HTTPX.post(
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

def score_pairs(pairs: list[dict], scorer) -> list[tuple[dict, float]]:
    """Pairs sharing the same `a` go in one request, like production."""
    groups = defaultdict(list)
    for pair in pairs:
        groups[json.dumps(pair["a"], sort_keys=True)].append(pair)
    scored = []
    for group in groups.values():
        scores = scorer(group[0]["a"], [p["b"] for p in group])
        scored += [(p, s) for p, s in zip(group, scores)]
    return scored


def summarize(scored: list[tuple[dict, float]], threshold: float) -> dict:
    """Accuracy over decided pairs; `unsure` pairs are listed but not counted."""
    decided = [(p, s) for p, s in scored if not p.get("unsure")]

    def accuracy(t):
        return sum((s >= t) == p["match"] for p, s in decided) / len(decided)

    best_t = max((t / 100 for t in range(5, 100, 5)), key=lambda t: (accuracy(t), -abs(t - threshold)))
    yes = [s for p, s in decided if p["match"]]
    no = [s for p, s in decided if not p["match"]]
    return {
        "pairs": len(decided), "accuracy": accuracy(threshold), "best_threshold": best_t,
        "best_accuracy": accuracy(best_t), "mean_match": sum(yes) / len(yes),
        "mean_nonmatch": sum(no) / len(no), "lowest_match": min(yes), "highest_nonmatch": max(no),
        "wrong": [{"id": p["id"], "kind": p["kind"], "expected": p["match"], "score": round(s, 3)}
                  for p, s in decided if (s >= threshold) != p["match"]],
        "unsure": {p["id"]: round(s, 3) for p, s in scored if p.get("unsure")},
        "scores": {p["id"]: round(s, 3) for p, s in scored},
    }


def rank(groups: list[dict], scorer, threshold: float) -> dict:
    """One request per group with all candidates, like production."""
    out = {"groups": len(groups), "top_is_good": 0, "all_correct": 0, "details": []}
    for g in groups:
        scores = scorer(g["me"], g["candidates"])
        best = max(range(len(scores)), key=lambda i: scores[i])
        top_ok = bool(g["candidates"][best].get("good")) and scores[best] >= threshold
        all_ok = all((s >= threshold) == bool(c.get("good")) for c, s in zip(g["candidates"], scores))
        out["top_is_good"] += top_ok
        out["all_correct"] += all_ok
        out["details"].append({"id": g["id"], "top_ok": top_ok, "all_ok": all_ok,
                               "scores": [round(s, 3) for s in scores],
                               "good": [bool(c.get("good")) for c in g["candidates"]]})
    return out


PAIR_FILES = ["match_pairs.json", "holdout_pairs.json", "extra_pairs.json"]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--offline", action="store_true", help="word-overlap baseline, no API calls")
    parser.add_argument("--variant", action="append", help="current, with-topic, strict, topic+strict, general")
    parser.add_argument("--no-calls", action="store_true", help="only use cached Jev answers; never call Jev")
    parser.add_argument("--threshold", type=float, default=float(os.environ.get("MATCH_THRESHOLD", "0.8")),
                        help="threshold used for 'right at' and the ranking checks")
    parser.add_argument("--pairs", action="append", help=f"pair file(s); default: {', '.join(PAIR_FILES)}")
    parser.add_argument("--no-ranking", action="store_true", help="skip ranking_groups.json")
    parser.add_argument("--json", help="also write full results here")
    args = parser.parse_args()

    files = args.pairs or PAIR_FILES
    sets = {f: json.loads((HERE / f).read_text())["pairs"] for f in files}
    groups = [] if args.no_ranking else json.loads((HERE / "ranking_groups.json").read_text())["groups"]
    total = sum(len(p) for p in sets.values())
    unsure = sum(bool(x.get("unsure")) for p in sets.values() for x in p)
    print(f"{total} pairs in {len(files)} file(s) ({unsure} marked unsure, not counted), "
          f"{len(groups)} ranking groups, threshold {args.threshold}\n")

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
            "with-topic": lambda me, others: ask_jev(key, me, others, True, None, sultan),
            "strict": lambda me, others: ask_jev(key, me, others, False, STRICT_CRITERIA, sultan),
            "topic+strict": lambda me, others: ask_jev(key, me, others, True, STRICT_CRITERIA, sultan),
            "general": lambda me, others: ask_jev(key, me, others, True, GENERAL_CRITERIA, sultan),
        }
        runs = [(v, variants[v]) for v in (args.variant or variants)]

    CACHED_HTTPX.allow_calls = not args.no_calls
    results = []
    for name, scorer in runs:
        per_file = {f: score_pairs(pairs, scorer) for f, pairs in sets.items()}
        everything = [x for scored in per_file.values() for x in scored]
        r = {"variant": name, "all": summarize(everything, args.threshold),
             "files": {f: summarize(scored, args.threshold) for f, scored in per_file.items()}}
        if groups:
            r["ranking"] = rank(groups, scorer, args.threshold)
        results.append(r)

        a = r["all"]
        print(f"== {name}: {a['accuracy']:.0%} right at {args.threshold} | best {a['best_accuracy']:.0%} at {a['best_threshold']:.2f} "
              f"| lowest match {a['lowest_match']:.2f}, highest non-match {a['highest_nonmatch']:.2f}")
        for f, fr in r["files"].items():
            print(f"   {f:20} {fr['pairs']:3} pairs: {fr['accuracy']:.0%} at {args.threshold}, best {fr['best_accuracy']:.0%} at {fr['best_threshold']:.2f}")
        for w in a["wrong"]:
            print(f"   wrong: {w['id']:40} expected {'match' if w['expected'] else 'no match':8} score {w['score']:.2f}  ({w['kind']})")
        if a["unsure"]:
            print("   unsure (not counted): " + ", ".join(f"{k} {v:.2f}" for k, v in a["unsure"].items()))
        if groups:
            g = r["ranking"]
            print(f"   ranking: top candidate right in {g['top_is_good']}/{g['groups']} groups, "
                  f"every candidate right in {g['all_correct']}/{g['groups']}")
            for d in g["details"]:
                if not d["all_ok"]:
                    marks = " ".join(f"{s:.2f}{'*' if good else ''}" for s, good in zip(d["scores"], d["good"]))
                    print(f"      {d['id']:18} {marks}   (* = should match)")
        print()
    if not args.offline:
        print(f"Jev calls made this run: {CACHED_HTTPX.calls} (the rest came from .jev_cache.json)")
    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=2))
        print(f"full results: {args.json}")


if __name__ == "__main__":
    main()
