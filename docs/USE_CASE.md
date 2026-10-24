# Use case walkthrough

A full run from sign-up to reconnecting, showing what happens in the database at each
step. Tables are in [`backend/schema.sql`](../backend/schema.sql); the rules behind them
are in [DATABASE.md](DATABASE.md).

## Cast

- **Sultan** (id 1): UBC, 18–24, Canada, bio "Rust + LeetCode"
- **Sara** (id 2): SFU, 18–24, Canada, bio "CS, interview prep"
- **Omar** (id 3): UofT, 18–24, Canada, bio "backend dev"

## Step 0: Sign up (once)

Each person installs the Tauri app, signs in, fills in a profile and turns sharing on.

```sql
INSERT INTO users (token, name, school, bio, age_group, country, sharing_enabled)
VALUES ('tok1', 'Sultan', 'UBC',  'Rust + LeetCode',    '18_24', 'CA', 1),
       ('tok2', 'Sara',   'SFU',  'CS, interview prep', '18_24', 'CA', 1),
       ('tok3', 'Omar',   'UofT', 'backend dev',        '18_24', 'CA', 1);
```

| Table | State |
|---|---|
| `users` | 3 rows |
| everything else | empty |

## Step 1: Sultan starts working (Monday 9:00)

On Sultan's Mac (nothing leaves it):

1. The app reads the frontmost app (Chrome), the URL (leetcode.com/problems/...) and the page HTML.
2. The blocklist check passes, since this isn't Gmail or banking.
3. The local AI summarizes the last 5 min into
   `"Solving LeetCode graph problems (BFS/DFS) in Python"` with topic `"LeetCode"`.

Push to the backend:

```sql
INSERT INTO curr_task (user_id, task_summary, topic)
VALUES (1, 'Solving LeetCode graph problems (BFS/DFS) in Python', 'LeetCode')
ON CONFLICT(user_id) DO UPDATE SET ...;
```

The push runs the Jev search for Sultan. The SQL prefilter returns everyone in
`curr_task` except Sultan who has sharing on, pushed in the last 11 min, is in a
compatible age group, isn't already connected with him on that topic, and didn't decline
him in the last day. Only Sultan is working, so there are no candidates and no match.

| Table | State |
|---|---|
| `curr_task` | Sultan: LeetCode |

## Step 2: Sara starts working (9:03)

Sara opens LeetCode. Her app summarizes and pushes:

```sql
INSERT INTO curr_task VALUES (2, 'Practicing LeetCode graph questions for interviews', 'LeetCode', ...);
```

The push runs the Jev search for Sara:

- Prefilter candidates: **Sultan** (working, same age group, not connected, never declined).
- Jev scores Sultan and Sara at **0.91**, above the 0.7 threshold, so it's a match.

The match is created for the first time, inside a transaction:

```sql
BEGIN IMMEDIATE;
-- does either of them already have a pending match? → no
INSERT INTO matches (user_a, user_b, topic, score) VALUES (1, 2, 'LeetCode', 0.91);
COMMIT;
```

`user_a = 1` because the smaller id always goes first. Both statuses start as `pending`.

| Table | State |
|---|---|
| `curr_task` | Sultan: LeetCode, Sara: LeetCode |
| `matches` | #41: (1, 2) LeetCode 0.91, pending / pending |

## Step 3: Both apps see the popup (9:03, within 20 s)

Every 20 seconds each app polls "any match waiting for me?" Sultan's poll returns match
41, with Sara's name, school, bio and current task joined in live from `users` and
`curr_task`.

Popup on Sultan's Mac:

> **Sara** · SFU · "CS, interview prep"
> Working on: *Practicing LeetCode graph questions for interviews*
> **[Work together]  [Not now]**

Sara's poll returns the same match 41 from her side:

> **Sultan** · UBC · "Rust + LeetCode"
> Working on: *Solving LeetCode graph problems (BFS/DFS) in Python*

Both apps now poll every ~2 s while the popup is open, so the other person's answer
shows up quickly.

## Step 4: Both accept (9:04)

Sultan clicks **Work together**:

```sql
UPDATE matches SET a_status = 'accepted' WHERE id = 41 AND user_a = 1;
```

His app shows "Waiting for Sara…". Sara clicks **Work together**:

```sql
UPDATE matches SET b_status = 'accepted' WHERE id = 41 AND user_b = 2;
```

The next poll from each app sees both accepted, so the Discord call link opens on both Macs.

| Table | State |
|---|---|
| `matches` | #41: accepted / accepted |

- **If someone declines instead:** `b_status = 'declined'`, the popup closes, and the
  prefilter skips this pair for 1 day.
- **If nobody answers:** after 2 min the popup query stops returning the match, and it
  expires without anyone doing anything.

## Step 5: After the call, "Keep in touch?" (9:50)

The call ends, and each app asks "Add Sara as a connection for LeetCode?" Both say yes:

```sql
INSERT INTO connections (user_a, user_b, topic, match_id) VALUES (1, 2, 'LeetCode', 41);
```

| Table | State |
|---|---|
| `connections` | #7: (1, 2) LeetCode, from match 41 |

They're now connected permanently on LeetCode. Match 41 stays as history.

## Step 6: Sultan quits for the day (17:00)

Sultan clicks Quit in the tray menu:

```sql
DELETE FROM curr_task                 WHERE user_id = 1;
DELETE FROM is_working_on_connections WHERE user_id = 1;
```

Nobody can be matched with Sultan, and no friend sees him as active, because he isn't working.

## Step 7: Next day, Sultan opens LeetCode (Tuesday 10:00)

His app pushes:

```sql
-- 1. save task
INSERT INTO curr_task VALUES (1, 'LeetCode dynamic programming practice', 'LeetCode', ...);

-- 2. reset my active rows: which of my connections are on topic 'LeetCode'?
DELETE FROM is_working_on_connections WHERE user_id = 1;
INSERT INTO is_working_on_connections (connection_id, user_id)
SELECT id, 1 FROM connections WHERE 1 IN (user_a, user_b) AND topic = 'LeetCode';
-- → inserts (7, 1)

-- 3. Jev search for new strangers (Sara is skipped, already connected on LeetCode)
```

| Table | State |
|---|---|
| `curr_task` | Sultan: LeetCode |
| `is_working_on_connections` | (connection 7, Sultan) |

## Step 8: Sara's app lights up Sultan's icon (10:00, within 20 s)

Sara's app is open and polling. Her "active friends" query finds connection 7 with Sultan
working on it. On Sara's tray panel:

> 🟢 **Sultan** is on **LeetCode** now · [Join]

She clicks Join, and they're working together again without any matching needed.

## Step 9: Sultan's laptop dies (10:30)

Sultan's battery dies, so the quit `DELETE` never runs and his rows stay in the database.
Sara still sees him as active.

Pushes are every 5 min, so after **11 min with no push** (about 2 missed), the cleanup at
the start of any poll removes him:

```sql
DELETE FROM is_working_on_connections WHERE seen_at < datetime('now', '-11 minutes');
```

At about 10:41, Sara's icon for Sultan turns grey. The Jev prefilter also skips his
`curr_task` row once `pushed_at` is more than 11 min old, so no stranger gets matched
with a dead laptop.

## Step 10: Race condition handled (any time)

Omar and Sara push at the exact same moment, and each search finds the other.

- Omar's push inserts match (2, 3), which succeeds.
- Sara's push tries to insert (2, 3) again, but the unique index
  `uq_matches_pending_pair` rejects it. The code ignores the error.

At the same time, Sultan's push also finds Sara, but the transaction check sees she
already has a pending match with Omar, so it skips her. Sara gets one popup, not three.

## Summary: which table each step touches

| Step | `users` | `curr_task` | `matches` | `connections` | `is_working_on_connections` |
|---|---|---|---|---|---|
| Sign up | write | | | | |
| Push (5 min) | read (filters) | write | write (if Jev matches) | read | write (own rows) |
| Poll (20 s) | read | read (popup task) | read | read | read + cleanup |
| Accept / decline | | | update | | |
| Keep in touch | | | | write | |
| Quit / pause | | delete | | | delete |
| Crash | | skipped after 11 min | | | deleted after 11 min |
