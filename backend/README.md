# Backend: Supabase + Python matcher

```
Mac app ──(supabase-js, user login)──▶ Supabase: profiles, activity, offers, connections, sessions
                                           ▲            │ Realtime: new offer → glass popup
Python matcher ──(service role key)────────┘            ▼
  every 20 s: eligible users → Jev scores pairs → Muse writes intros → create_offer()
```

- **Schema:** `supabase/migrations/` (Postgres port of `schema.sql`, plus RLS and SQL functions). `schema.sql` is the original SQLite version.
- **Matcher:** `backend/matcher/`, Python 3.12. It needs no inbound port, so any always-on host works.
- **No keys?** The matcher still runs: topic-overlap scoring instead of Jev, template intros instead of Muse.

## Run locally

```sh
# 1. Local Supabase (Docker): applies migrations + supabase/seed.sql (3 demo users)
npx supabase start
npx supabase db reset            # re-run migrations + seed any time

# 2. Matcher
cd backend
python3.12 -m venv .venv && .venv/bin/pip install -r requirements.txt
cp .env.example .env             # SUPABASE_URL + service_role key from `npx supabase status`
.venv/bin/python -m matcher --once   # one pass; drop --once to loop
.venv/bin/python -m unittest discover tests
```

With the seed data, one pass offers Alex + Sam a match on graph algorithms; Jo (Figma) is left alone.

## How the Mac app uses it

All through `@supabase/supabase-js` with the **anon key** and the user's login. Never ship the service role key.

| When | Call |
|---|---|
| Onboarding | `supabase.auth.signUp(...)`, then `from('profiles').insert({ id, name, school, bio, field, discord_handle, sharing_enabled })` |
| Every 5-min summary | `from('activity').upsert({ user_id, summary, topic, subtopics, context, activity_type, focus, stuck, updated_at: new Date() })` |
| Pause / go offline | `from('activity').delete().eq('user_id', id)` |
| Listen for matches | `channel('offers').on('postgres_changes', { event: 'INSERT', schema: 'public', table: 'offers' }, show_popup)` |
| Accept / Not now | `rpc('respond_to_offer', { p_offer_id, p_accept })`. Both accept → connection + 25-min session created |
| After the session | `rpc('answer_friend', { p_connection_id, p_wants })` |

The popup text is `intro_a` if you are `user_a`, else `intro_b`. Offers expire after 2 minutes (`expires_at`).

## Privacy rules enforced by the database

- `activity` (your full summary) is readable only by you and the matcher.
- `offers` holds only what both people may see: the shared topic and each person's intro.
- `match_log` (the summaries a decision used) is matcher-only.
- Offer and connection changes go through `respond_to_offer` / `answer_friend`, so you can only answer your own side.

## Deploy

1. Create a Supabase project, then `npx supabase link --project-ref <ref>` and `npx supabase db push`.
2. Run the matcher on any always-on host with the env vars from `.env.example`. Start command: `python -m matcher`, working directory `backend/`.
