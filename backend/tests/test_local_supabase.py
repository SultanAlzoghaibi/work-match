"""End-to-end checks against local Supabase (npx supabase start + db reset).
Skipped unless LOCAL_SUPABASE=1. Signs in as the seed users with the anon key,
the same way the Mac app does.

    npx supabase db reset && LOCAL_SUPABASE=1 .venv/bin/python -m unittest tests.test_local_supabase -v
"""

import os
import unittest

from supabase import create_client

from matcher.config import Config
from matcher.intro import TemplateIntroWriter
from matcher.jev import FallbackScorer
from matcher.match import Thresholds, run_once
from matcher.store import SupabaseStore

URL = "http://127.0.0.1:54321"
# Supabase's fixed local-development anon key (same on every machine).
ANON_KEY = ("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6ImFub24iLCJleHAiOjE5ODM4MTI5OTZ9"
            ".CRXP1A7WOeoJeXxjNni43kdQwgnWNReilDMblYTn_I0")
ALEX, SAM, JO = ("11111111-1111-1111-1111-111111111111", "22222222-2222-2222-2222-222222222222",
                 "33333333-3333-3333-3333-333333333333")


def login(email):
    client = create_client(URL, ANON_KEY)
    client.auth.sign_in_with_password({"email": email, "password": "password"})
    return client


@unittest.skipUnless(os.environ.get("LOCAL_SUPABASE") == "1", "needs local Supabase")
class LocalSupabaseTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cfg = Config.from_env()
        cls.store = SupabaseStore(cfg.supabase_url, cfg.supabase_service_role_key)
        run_once(cls.store, FallbackScorer(), TemplateIntroWriter(), Thresholds())
        cls.alex, cls.sam, cls.jo = login("alex@example.com"), login("sam@example.com"), login("jo@example.com")

    def offers(self, client):
        return client.table("offers").select("*").execute().data

    def test_1_privacy(self):
        offer = self.offers(self.alex)[0]
        self.assertEqual({offer["user_a"], offer["user_b"]}, {ALEX, SAM})
        self.assertEqual(len(self.offers(self.sam)), 1)
        self.assertEqual(self.offers(self.jo), [], "Jo must not see Alex and Sam's offer")

        # Activity: only your own row.
        rows = self.alex.table("activity").select("user_id").execute().data
        self.assertEqual([r["user_id"] for r in rows], [ALEX])
        # match_log (the summaries behind a decision): nobody but the matcher.
        self.assertEqual(self.alex.table("match_log").select("*").execute().data, [])
        # Profiles: yourself plus people you were matched with, not Jo.
        names = {p["name"] for p in self.alex.table("profiles").select("name").execute().data}
        self.assertEqual(names, {"Alex", "Sam"})
        # Matcher-only functions are off limits.
        with self.assertRaises(Exception):
            self.alex.rpc("eligible_users").execute()
        # Can't edit offers directly.
        self.alex.table("offers").update({"b_status": "accepted"}).eq("id", offer["id"]).execute()
        self.assertEqual(self.offers(self.sam)[0]["b_status"], "pending")

    def test_2_accept_creates_connection_and_session(self):
        offer_id = self.offers(self.alex)[0]["id"]
        with self.assertRaises(Exception):
            self.jo.rpc("respond_to_offer", {"p_offer_id": offer_id, "p_accept": True}).execute()

        self.alex.rpc("respond_to_offer", {"p_offer_id": offer_id, "p_accept": True}).execute()
        with self.assertRaises(Exception):  # can't answer twice
            self.alex.rpc("respond_to_offer", {"p_offer_id": offer_id, "p_accept": False}).execute()
        result = self.sam.rpc("respond_to_offer", {"p_offer_id": offer_id, "p_accept": True}).execute().data
        self.assertIsNotNone(result["connection_id"])

        connection = self.alex.table("connections").select("*").execute().data[0]
        self.assertEqual((connection["status"], connection["topic"]), ("trial", "graph algorithms"))
        sessions = self.sam.table("cowork_sessions").select("*").execute().data
        self.assertEqual(len(sessions), 1)
        self.assertIsNone(sessions[0]["ended_at"])

        # In a session now, so neither is matched again.
        self.assertEqual(run_once(self.store, FallbackScorer(), TemplateIntroWriter(), Thresholds()), [])

    def test_3_friends(self):
        connection_id = self.alex.table("connections").select("id").execute().data[0]["id"]
        self.alex.rpc("answer_friend", {"p_connection_id": connection_id, "p_wants": True}).execute()
        result = self.sam.rpc("answer_friend", {"p_connection_id": connection_id, "p_wants": True}).execute().data
        self.assertEqual(result["status"], "friends")


if __name__ == "__main__":
    unittest.main()
