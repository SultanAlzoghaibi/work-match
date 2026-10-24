"""API flow against a real Postgres loaded with backend/schema.sql.
Skipped unless TEST_DATABASE_URL is set. Every table is emptied first, so
never point it at a database with real data.

    TEST_DATABASE_URL=postgresql://postgres:postgres@127.0.0.1:54322/wm_api_test \
        .venv/bin/python -m unittest tests.test_api -v
"""

import os
import unittest

TEST_DB = os.environ.get("TEST_DATABASE_URL")
if TEST_DB:
    os.environ["DATABASE_URL"] = TEST_DB
    os.environ["OPENROUTER_API_KEY"] = ""  # topic-overlap fallback, no network

    import psycopg
    from fastapi.testclient import TestClient

    from app.main import app

ALEX = {"name": "Alex", "school": "UC Berkeley", "bio": "CS junior", "age_group": "18_24", "sharing_enabled": True}
SAM = {"name": "Sam", "school": "UC Berkeley", "bio": "CS170 midterm prep", "age_group": "18_24", "sharing_enabled": True}
JO = {"name": "Jo", "school": "CCA", "bio": "Designer", "age_group": "18_24", "sharing_enabled": True}
KID = {"name": "Kid", "school": "High school", "bio": "Learning Python", "age_group": "under_18", "sharing_enabled": True}

GRAPHS = "graph algorithms"


@unittest.skipUnless(TEST_DB, "set TEST_DATABASE_URL")
class ApiFlowTest(unittest.TestCase):
    def setUp(self):
        with psycopg.connect(TEST_DB) as conn:
            conn.execute("truncate users, curr_task, matches, connections, is_working_on_connections restart identity cascade")
        self.client = TestClient(app)
        self.client.__enter__()

    def tearDown(self):
        self.client.__exit__(None, None, None)

    def signup(self, profile):
        r = self.client.post("/users", json=profile)
        self.assertEqual(r.status_code, 201, r.text)
        return {"Authorization": f"Bearer {r.json()['token']}"}

    def push(self, auth, summary, topic=GRAPHS):
        r = self.client.post("/push", json={"task_summary": summary, "topic": topic}, headers=auth)
        self.assertEqual(r.status_code, 200, r.text)

    def poll(self, auth):
        r = self.client.get("/poll", headers=auth)
        self.assertEqual(r.status_code, 200, r.text)
        return r.json()

    def test_auth(self):
        self.assertEqual(self.client.get("/me").status_code, 401)
        self.assertEqual(self.client.get("/me", headers={"Authorization": "Bearer nope"}).status_code, 401)
        auth = self.signup(ALEX)
        self.assertEqual(self.client.get("/me", headers=auth).json()["name"], "Alex")
        with psycopg.connect(TEST_DB) as conn:
            stored = conn.execute("select token from users").fetchone()[0]
        self.assertNotIn(stored, auth["Authorization"], "raw token must not be stored")

    def test_match_poll_accept(self):
        alex, sam, jo = self.signup(ALEX), self.signup(SAM), self.signup(JO)
        self.push(jo, "Designing an onboarding flow in Figma.", "mobile app design")
        self.push(alex, "Implementing Dijkstra in Python for CS170 homework.")
        self.push(sam, "Watching a lecture on shortest-path algorithms.")  # runs the search for Sam

        [m] = self.poll(alex)["matches"]
        self.assertEqual((m["other_name"], m["topic"]), ("Sam", GRAPHS))
        self.assertEqual(m["task_summary"], "Watching a lecture on shortest-path algorithms.")
        self.assertEqual(self.poll(jo)["matches"], [])

        r = self.client.post(f"/matches/{m['id']}/respond", json={"accept": True}, headers=jo)
        self.assertEqual(r.status_code, 409, "Jo can't answer Alex and Sam's match")

        r = self.client.post(f"/matches/{m['id']}/respond", json={"accept": True}, headers=alex)
        self.assertFalse(r.json()["start_call"])
        self.assertEqual(self.client.post(f"/matches/{m['id']}/respond", json={"accept": True}, headers=alex).status_code, 409)
        r = self.client.post(f"/matches/{m['id']}/respond", json={"accept": True}, headers=sam)
        self.assertTrue(r.json()["start_call"])
        self.assertTrue(self.poll(alex)["matches"][0]["start_call"])

    def test_one_popup_at_a_time(self):
        alex, sam, extra = self.signup(ALEX), self.signup(SAM), self.signup({**ALEX, "name": "Riley"})
        self.push(alex, "Dijkstra homework.")
        self.push(sam, "Shortest-path lecture.")
        self.push(extra, "Graph algorithms practice.")
        self.assertEqual(len(self.poll(alex)["matches"]), 1)
        self.assertEqual(self.poll(extra)["matches"], [])

    def test_minors_only_with_minors(self):
        alex, kid = self.signup(ALEX), self.signup(KID)
        self.push(alex, "Dijkstra homework.")
        self.push(kid, "Learning graph algorithms.")
        self.assertEqual(self.poll(kid)["matches"], [])

    def test_decline_and_sharing_off(self):
        alex, sam = self.signup(ALEX), self.signup(SAM)
        self.push(alex, "Dijkstra homework.")
        self.push(sam, "Shortest-path lecture.")
        [m] = self.poll(alex)["matches"]
        self.client.post(f"/matches/{m['id']}/respond", json={"accept": False}, headers=sam)
        self.assertEqual(self.poll(alex)["matches"], [], "declined match disappears")
        self.push(alex, "Still on Dijkstra.")
        self.assertEqual(self.poll(alex)["matches"], [], "no rematch within a day")

        quiet = self.signup({**SAM, "name": "Quiet", "sharing_enabled": False})
        self.push(quiet, "Graph algorithms too.")
        self.assertEqual(self.poll(quiet)["matches"], [])

    def test_quit(self):
        alex, sam = self.signup(ALEX), self.signup(SAM)
        self.push(alex, "Dijkstra homework.")
        self.client.post("/quit", headers=alex)
        self.push(sam, "Shortest-path lecture.")
        self.assertEqual(self.poll(sam)["matches"], [], "quit users aren't matched")


if __name__ == "__main__":
    unittest.main()
