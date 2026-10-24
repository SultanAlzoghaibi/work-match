"""Matcher logic with an in-memory store. Run: python -m unittest discover tests"""

import json
import unittest

import httpx

from matcher.intro import MuseIntroWriter, TemplateIntroWriter
from matcher.jev import FallbackScorer, JevScorer, Score
from matcher.match import Thresholds, best_candidate, run_once
from matcher.people import Person

ALEX = Person("a", "Alex", "Implementing Dijkstra in Python for CS170 homework.", "graph algorithms",
              ["dijkstra", "shortest paths"], "CS170 homework", "coding", "high", True)
SAM = Person("b", "Sam", "Watching a lecture on shortest-path algorithms.", "graph algorithms",
             ["shortest paths", "bellman-ford"], "CS170", "studying", "medium", False)
JO = Person("c", "Jo", "Designing an onboarding flow in Figma.", "mobile app design",
            ["onboarding", "figma"], None, "designing", "medium", False)


class FakeStore:
    def __init__(self, people):
        self.people = people
        self.offers = []

    def eligible_users(self):
        return list(self.people)

    def candidates(self, user_id):
        return [p for p in self.people if p.user_id != user_id]

    def recent_outcomes(self):
        return []

    def create_offer(self, **kw):
        self.offers.append(kw)
        return len(self.offers)


class FixedScorer:
    model = "fixed"

    def __init__(self, table):
        self.table = table  # {(me, other): (match, interrupt)}

    def score_many(self, me, others, outcomes):
        return [Score(o, *self.table.get((me.user_id, o.user_id), (0.0, 1.0)), self.model) for o in others]


class RunOnceTest(unittest.TestCase):
    def test_pairs_best_match_once(self):
        store = FakeStore([ALEX, SAM, JO])
        scorer = FixedScorer({("a", "b"): (0.9, 0.8), ("a", "c"): (0.2, 0.9), ("c", "b"): (0.95, 0.9)})
        created = run_once(store, scorer, TemplateIntroWriter(), Thresholds())
        self.assertEqual(created, [1])
        offer = store.offers[0]
        self.assertEqual((offer["one"].name, offer["two"].name), ("Alex", "Sam"))
        self.assertEqual(offer["topic"], "graph algorithms")
        self.assertIn("Sam", offer["intro_one"])
        self.assertIn("Alex", offer["intro_two"])

    def test_no_offer_below_thresholds(self):
        store = FakeStore([ALEX, SAM])
        scorer = FixedScorer({("a", "b"): (0.6, 0.9), ("b", "a"): (0.9, 0.2)})
        self.assertEqual(run_once(store, scorer, TemplateIntroWriter(), Thresholds()), [])

    def test_needs_two_people(self):
        self.assertEqual(run_once(FakeStore([ALEX]), FixedScorer({}), TemplateIntroWriter(), Thresholds()), [])

    def test_best_candidate_picks_highest_passing(self):
        scores = [Score(SAM, 0.8, 0.9, "x"), Score(JO, 0.95, 0.1, "x")]
        self.assertIs(best_candidate(scores, Thresholds()).candidate, SAM)


class FallbackScorerTest(unittest.TestCase):
    def test_related_beats_unrelated(self):
        scores = {s.candidate.name: s.match for s in FallbackScorer().score_many(ALEX, [SAM, JO], [])}
        self.assertGreater(scores["Sam"], 0.7)
        self.assertLess(scores["Jo"], 0.2)


def mock_client(handler):
    return httpx.Client(transport=httpx.MockTransport(handler))


class JevScorerTest(unittest.TestCase):
    def test_sends_questions_and_parses_noul(self):
        seen = {}

        def handler(request):
            seen["body"] = json.loads(request.content)
            seen["auth"] = request.headers["authorization"]
            return httpx.Response(200, json={
                "good_match": {"type": "noul", "noul": 0.91},
                "a_open": {"type": "noul", "noul": 0.7},
                "b_open": {"type": "noul", "noul": 0.6},
                "usage": {"cost": 0.0001},
            })

        scorer = JevScorer("key", "typesafe/jev-1.14")
        scorer._client = mock_client(handler)
        scorer._client.headers["Authorization"] = "Bearer key"
        score = scorer.score_pair(ALEX, SAM, [])
        self.assertEqual((score.match, score.interrupt_ok), (0.91, 0.6))
        self.assertEqual(seen["auth"], "Bearer key")
        self.assertEqual(set(seen["body"]["questions"]), {"good_match", "a_open", "b_open"})
        self.assertNotIn("Alex", " ".join(seen["body"]["state"]))  # names are not sent to Jev

    def test_failed_pair_is_skipped(self):
        scorer = JevScorer("key", "m")
        scorer._client = mock_client(lambda r: httpx.Response(500))
        self.assertEqual(scorer.score_many(ALEX, [SAM], []), [])


class MuseIntroWriterTest(unittest.TestCase):
    def test_parses_json_reply(self):
        reply = '```json\n{"topic": "shortest paths", "intro_a": "Sam too!", "intro_b": "Alex too!"}\n```'
        writer = MuseIntroWriter("key", "https://example.test/v1", "muse")
        writer._client = mock_client(lambda r: httpx.Response(200, json={"choices": [{"message": {"content": reply}}]}))
        intro = writer.write(ALEX, SAM)
        self.assertEqual((intro.topic, intro.intro_a, intro.model), ("shortest paths", "Sam too!", "muse"))

    def test_bad_reply_falls_back_to_template(self):
        writer = MuseIntroWriter("key", "https://example.test/v1", "muse")
        writer._client = mock_client(lambda r: httpx.Response(200, json={"choices": [{"message": {"content": "hi!"}}]}))
        self.assertEqual(writer.write(ALEX, SAM).model, "fallback:template")


if __name__ == "__main__":
    unittest.main()
