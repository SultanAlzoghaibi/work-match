# Database

The schema lives in [`backend/schema.sql`](../backend/schema.sql) and runs on Supabase
(Postgres). `users.id` is a UUID (`gen_random_uuid()`), so every user reference is a
`uuid`. Match and connection ids stay `bigint`. The examples in USE_CASE.md use small
numbers like `1` and `2` for readability.

## Match vs connection

These are two different things. Don't mix them up.

| | Match | Connection |
|---|---|---|
| What | Jev pairs two people working on the same thing right now | Two people choose to keep each other after a match |
| Result | One call together | Each sees an "active" icon when the other is on that topic |
| Lifetime | One-time; the popup expires after 2 min | Permanent |
| Table | `matches` | `connections` (+ `is_working_on_connections` for the icon) |

Flow:

1. Jev finds a pair, which inserts a `matches` row. That row is the "match occurred" flag.
2. Both apps show the popup. Each person answers on their own (`a_status`, `b_status`).
3. Both accept: the call starts.
4. Both say "keep in touch": a `connections` row is created for that topic.
5. Later, whenever one of them pushes a task on that topic, a row lands in
   `is_working_on_connections` and the other person's icon lights up.

A pair can have many matches, and can be connected on several topics (one row per topic).

## Tables

| # | Table | One row means | Lifetime |
|---|---|---|---|
| 1 | `users` | one person | permanent |
| 2 | `curr_task` | what one person is doing now (`task_summary` + `topic`) | overwritten every push, deleted on quit |
| 2b | `curr_task_history` | one past push (`task_summary` + `topic` + `pushed_at`) | permanent; for debugging and looking back at past work |
| 3 | `matches` | Jev paired two people | permanent history |
| 4 | `connections` | two people follow each other on a topic | permanent |
| 5 | `is_working_on_connections` | someone is on a connection's topic right now | temporary, usually empty |

Why `is_working_on_connections` is its own table and not an `is_active` column on
`connections`: a connection has two people, so one flag can't say who is active. It also
keeps the table small, and each user only writes their own rows.

## Timing

| What | How often | Does |
|---|---|---|
| Push | every 5 min | app sends `task_summary` + `topic`; server rewrites `curr_task` and the user's `is_working_on_connections` rows, then runs the Jev search for this user |
| Poll | every 20 s (every ~2 s while a popup is open) | app reads pending matches and active connections |
| Match popup | expires after 2 min | filtered by `matches.created_at` |
| Active icon | expires after 11 min without a push | rows with an old `seen_at` are deleted at the start of each poll |
| Quit / pause | on event | app deletes its `curr_task` and `is_working_on_connections` rows |

The 11-minute timeout covers a crash (the quit `DELETE` never runs): after about 2 missed
pushes the row is treated as gone. The Jev prefilter skips `curr_task` rows with an old
`pushed_at` for the same reason.

## Race conditions

Two pushes at the same moment can find the same pair, or match one person twice.

- `uq_matches_pending_pair` allows only one pending match per pair; a second insert fails
  and is ignored.
- The "does either person already have a pending match?" check and the insert run in one
  transaction (`BEGIN IMMEDIATE` in SQLite; a locking database function on Supabase).

## Walkthrough

See [USE_CASE.md](USE_CASE.md) for a full run from sign-up to reconnecting.

Polling instead of WebSockets: the schema is the same either way, so push can be added
later (for example with Supabase Realtime).

All the push, poll and accept queries are at the bottom of `backend/schema.sql`.

## Privacy

- The Tauri app filters and summarizes on the device. Only `task_summary` and `topic`
  are stored on the server, never raw URLs, window titles or page HTML.
- The match popup shows the other person's name, school, bio and current `task_summary`
  (read live from `curr_task`, not copied). The on-device AI should write summaries
  knowing a stranger may see them: no file names, client names or personal details.
- `matches` and `connections` keep only `topic`, not summaries.
- `curr_task_history` **does** keep every pushed `task_summary` permanently (added for
  debugging and a "past work" view). Only the backend reads it (RLS on, no policies); delete
  a user's rows if they ask, and consider a retention limit before launch.
- Connections see only the topic in the active icon, not the summary.
- On Supabase, turn on row-level security so users can't read `curr_task` directly. Only
  the backend reads it.
- Under-18 users are matched only with each other.
- Tokens: store a hash, not the raw value (TODO).
