# API

The Mac app's only way to reach the backend. The server (`backend/app`) talks
to Supabase using the schema in `backend/schema.sql`; the app never touches the
database directly. Flow and timing follow [DATABASE.md](DATABASE.md).

- **Base URL:** `https://work-match-api.onrender.com` (Render), or `http://127.0.0.1:8000` locally
- **Auth:** `Authorization: Bearer <token>` on every call except `POST /users` and `GET /health`
- **Errors:** `401` bad or missing token, `409` match gone/expired/already answered, `422` invalid body

The free Render plan sleeps after 15 minutes without traffic, and the first
request after that takes 30-60 s. Polling keeps it awake while anyone has the
app open.

## `POST /users`: sign up

```json
{ "name": "Alex", "school": "UC Berkeley", "bio": "CS junior",
  "age_group": "18_24", "country": "US", "sharing_enabled": true }
```
→ `201`
```json
{ "id": 7, "token": "kq3...Zx" }
```
The token is returned **only here**. Store it in the local DB (`curr_user_profile.token`).
The server keeps only its SHA-256 hash.

`age_group` is one of `under_18`, `18_24`, `25_34`, `35_44`, `45_plus`.
Under-18 users are matched only with each other.

## `GET /me`, `PATCH /me`

`PATCH` takes any subset of the sign-up fields, e.g. `{ "sharing_enabled": false }`.
Both return the profile.

## `POST /push`: every 5 minutes

```json
{ "task_summary": "Implementing Dijkstra in Python for CS170 homework.", "topic": "graph algorithms" }
```
→ `{ "ok": true }`

Replaces your `curr_task`, lights up your connections on this topic, then runs
the Jev search for you in the background. A match shows up on the next poll.
`task_summary` is shown to the person you match with, so the on-device summary
must not contain file names, client names, or personal details.

## `GET /poll`: every 20 seconds (every ~2 s while a popup is open)

```json
{
  "matches": [
    { "id": 41, "topic": "graph algorithms", "score": 0.91,
      "other_name": "Sam", "school": "UC Berkeley", "bio": "CS170 midterm prep",
      "task_summary": "Watching a lecture on shortest-path algorithms.",
      "my_status": "pending", "their_status": "accepted",
      "start_call": false, "created_at": "2026-10-24T08:03:12Z" }
  ],
  "active_connections": [
    { "connection_id": 3, "topic": "LeetCode", "friend_name": "Sara" }
  ]
}
```
`matches` holds popups from the last 2 minutes that nobody declined. When
`start_call` is `true`, both accepted: open the call.

## `POST /matches/{id}/respond`

```json
{ "accept": true }
```
→ `{ "a_status": "accepted", "b_status": "pending", "start_call": false }`

Answers your side only, once, within 2 minutes.

## `POST /quit`: on quit or pause

→ `{ "ok": true }`. Deletes your `curr_task` and active rows, so nobody is matched with you.

## `GET /health`

→ `{ "ok": true, "scorer": "typesafe/jev-1.14" }` (`fallback:topic` when no Jev key is set)

## Not built yet

- **Keep in touch → connection.** The schema has no columns for each person's
  "keep in touch?" answer, so connections can't be created yet. Proposed fix:
  `a_keep` / `b_keep` booleans on `matches`.
- **Call link.** `users` has no Discord handle or call URL.
