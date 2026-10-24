"""Supabase access for the matcher. Uses the service role key and the SQL
functions in supabase/migrations (eligible_users, match_candidates, ...)."""

from supabase import Client, create_client

from .people import Person


class SupabaseStore:
    def __init__(self, url: str, service_role_key: str):
        self._db: Client = create_client(url, service_role_key)

    def eligible_users(self) -> list[Person]:
        rows = self._db.rpc("eligible_users").execute().data or []
        if not rows:
            return []
        ids = [r["user_id"] for r in rows]
        profiles = {
            p["id"]: p
            for p in self._db.table("profiles").select("id,name,school,bio,field").in_("id", ids).execute().data
        }
        people = []
        for r in rows:
            p = profiles.get(r["user_id"], {})
            people.append(Person.from_row({**r, **{k: v for k, v in p.items() if k != "id"}}))
        return people

    def candidates(self, user_id: str) -> list[Person]:
        rows = self._db.rpc("match_candidates", {"me": user_id}).execute().data or []
        return [Person.from_row(r) for r in rows]

    def recent_outcomes(self) -> list[dict]:
        return self._db.rpc("recent_outcomes").execute().data or []

    def create_offer(self, *, one: Person, two: Person, topic: str, score: float,
                     intro_one: str, intro_two: str, interrupt_ok: float,
                     decision_model: str, intro_model: str) -> int | None:
        return self._db.rpc("create_offer", {
            "p_one": one.user_id,
            "p_two": two.user_id,
            "p_activity": topic,
            "p_score": score,
            "p_intro_one": intro_one,
            "p_intro_two": intro_two,
            "p_summary_one": one.summary,
            "p_summary_two": two.summary,
            "p_interrupt_ok": interrupt_ok,
            "p_decision_model": decision_model,
            "p_intro_model": intro_model,
        }).execute().data
