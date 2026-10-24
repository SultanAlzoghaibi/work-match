-- work-match: backend database schema (Supabase / Postgres)
-- Paste into the Supabase SQL editor, or run as a migration.
--
-- Match      = Jev pairs two people -> popup -> one call together.
-- Connection = after a call, both choose to follow each other on that topic.
-- See docs/DATABASE.md and docs/USE_CASE.md.
--
-- The Tauri app filters raw activity on the device. Only task_summary and topic
-- ever reach this database.
--
-- Timing:
--   push (every 5 min)  : app sends task_summary + topic -> curr_task, then Jev search runs
--   poll (every 20 s)   : app reads match popups + active connections
--   match popup expires : 2 min
--   active row expires  : 11 min without a push (about 2 missed pushes = app gone)

-- Profile pictures live in Supabase Storage, not in a table. One-time setup in the
-- dashboard: Storage -> New bucket "avatars", public, 2 MB limit, image/jpeg,
-- image/png, image/webp. The backend uploads {user_id}.jpg with the service-role key.
-- Existing DB: alter table users add column avatar_url text;
--              alter table users add column career text;

-- 1. Who someone is. Permanent.
create table users (
    id              uuid        primary key default gen_random_uuid(),   -- the app may also pick it
    token           text        not null unique,                 -- TODO: store a hash
    username        text        unique,                          -- lowercase; sign-in name
    password_hash   text,                                        -- scrypt$salt$hash
    name            text        not null,
    school          text,
    career          text,                                        -- e.g. 'Software engineer', 'CS student'
    bio             text,                                        -- shown in match popup
    discord_handle  text,                                        -- used to start the call
    avatar_url      text,                                        -- public URL in Storage bucket 'avatars'; null = show initial
    age_group       text        check (age_group in ('under_18','18_24','25_34','35_44','45_plus')),
    country         char(2),                                     -- ISO code, e.g. 'CA'
    languages       text[]      not null default '{}',           -- e.g. {English,Arabic}; used by filters
    sharing_enabled boolean     not null default false,
    created_at      timestamptz not null default now()
);

-- 1b. Who each person is willing to be matched with. One row per user; no row =
--     no filters. Matching is mutual: both people's filters must allow the pair.
create table user_filters (
    user_id         uuid        primary key references users(id) on delete cascade,
    age_groups      text[]      not null default '{}',           -- allowed age groups; empty = any
    countries       char(2)[]   not null default '{}',           -- allowed countries (ISO); empty = anywhere
    shared_language boolean     not null default false,          -- only people who share a language with me
    updated_at      timestamptz not null default now()
);

-- 2. What each person is doing right now. One row per user, overwritten every push,
--    deleted on app quit or pause.
create table curr_task (
    user_id      uuid        primary key references users(id) on delete cascade,
    task_summary text        not null,                           -- built on the device, shown to matches
    topic        text,                                           -- short label, e.g. 'LeetCode'
    pushed_at    timestamptz not null default now()
);

-- 2b. Every push, kept as history (curr_task only holds the latest one). For
--     debugging and for looking back at past work. Never deleted by quit/pause.
create table curr_task_history (
    id           bigint generated always as identity primary key,
    user_id      uuid        not null references users(id) on delete cascade,
    task_summary text        not null,
    topic        text,
    pushed_at    timestamptz not null default now()
);

create index idx_curr_task_history_user on curr_task_history (user_id, pushed_at desc);

-- 3. Jev paired two people. A new row = match occurred (created once, starts pending).
--    Each side answers the popup separately.
create table matches (
    id         bigint generated always as identity primary key,
    user_a     uuid        not null references users(id) on delete cascade,
    user_b     uuid        not null references users(id) on delete cascade,
    topic      text        not null,                             -- no summaries stored here
    score      real        check (score between 0 and 1),        -- Jev P(good match)
    a_status   text        not null default 'pending' check (a_status in ('pending','accepted','declined')),
    b_status   text        not null default 'pending' check (b_status in ('pending','accepted','declined')),
    created_at timestamptz not null default now(),               -- popup expires after 2 min
    room_id    text,                                             -- Jitsi room name, set when both accept; the apps join it
    a_keep     boolean,                                          -- "keep in touch?" after both accept;
    b_keep     boolean,                                          --   NULL until answered, both true -> connection
    check (user_a < user_b)                                      -- store each pair one way
);

create index idx_matches_user_a on matches (user_a, created_at);
create index idx_matches_user_b on matches (user_b, created_at);

-- Race fix: only one pending match per pair. If two pushes find the same pair at
-- once, the second insert fails (create_match below ignores it).
create unique index uq_matches_pending_pair
    on matches (user_a, user_b)
    where a_status = 'pending' and b_status = 'pending';

-- 4. Two people follow each other on a topic. Permanent.
create table connections (
    id         bigint generated always as identity primary key,
    user_a     uuid        not null references users(id) on delete cascade,
    user_b     uuid        not null references users(id) on delete cascade,
    topic      text        not null,
    match_id   bigint      references matches(id) on delete set null,   -- the call that started it
    created_at timestamptz not null default now(),
    unique (user_a, user_b, topic),
    check (user_a < user_b)
);

create index idx_connections_user_b on connections (user_b);   -- user_a covered by unique

-- 5. Someone is working on a connection's topic right now -> friend's icon lights up.
--    Usually empty. Rows are rewritten on every push and removed on quit or timeout.
create table is_working_on_connections (
    connection_id bigint      not null references connections(id) on delete cascade,
    user_id       uuid        not null references users(id)       on delete cascade,
    seen_at       timestamptz not null default now(),
    primary key (connection_id, user_id)
);

-- 6. Observability for the Jev search: one jev_runs row per match search (every
--    push), one jev_comparisons row per candidate Jev scored in it. Written by
--    matching.find_match; logging failures never block a match.
create table jev_runs (
    id              bigint generated always as identity primary key,
    user_id         uuid        not null references users(id) on delete cascade,
    my_summary      text        not null,
    my_topic        text,
    threshold       real        not null,                        -- MATCH_THRESHOLD used
    candidate_count int         not null default 0,             -- after the SQL prefilter
    request         jsonb,                                       -- exact body sent to Jev (state + questions)
    response        jsonb,                                       -- raw "answers" from Jev
    duration_ms     int,                                         -- Jev round trip
    outcome         text        not null check (outcome in
                        ('matched','below_threshold','all_busy','no_candidates','sharing_off','jev_error')),
    error           text,                                        -- Jev error / skip reason
    match_id        bigint      references matches(id) on delete set null,
    matched_user_id uuid        references users(id) on delete set null,
    created_at      timestamptz not null default now()
);
create index idx_jev_runs_user on jev_runs (user_id, created_at desc);
create index idx_jev_runs_created on jev_runs (created_at desc);

create table jev_comparisons (
    id                bigint generated always as identity primary key,
    run_id            bigint      not null references jev_runs(id) on delete cascade,
    person_label      text        not null,                      -- "Person 3": how Jev saw them
    candidate_user_id uuid        references users(id) on delete set null,
    candidate_name    text,
    candidate_summary text        not null,
    candidate_topic   text,
    score             real,                                      -- P(good match); null = Jev gave no answer
    rank              int,                                       -- 1 = best score in this run
    decision          text        not null check (decision in
                          ('matched','below_threshold','busy','not_tried','no_answer'))
);
create index idx_jev_comparisons_run on jev_comparisons (run_id);
create index idx_jev_comparisons_candidate on jev_comparisons (candidate_user_id);

-- Only the backend (service role) touches these tables. RLS on with no policies
-- blocks direct reads from the app's anon key.
alter table users                     enable row level security;
alter table user_filters              enable row level security;
alter table curr_task                 enable row level security;
alter table curr_task_history         enable row level security;
alter table matches                   enable row level security;
alter table connections               enable row level security;
alter table is_working_on_connections enable row level security;
alter table jev_runs                  enable row level security;
alter table jev_comparisons           enable row level security;

-- Create a match safely. Returns the new match id, or null if skipped.
-- The advisory locks make concurrent pushes involving the same person wait for each
-- other, so one person never gets two popups at once.
create function create_match(p_a uuid, p_b uuid, p_topic text, p_score real)
returns bigint
language plpgsql
as $$
declare
    lo uuid := least(p_a, p_b);
    hi uuid := greatest(p_a, p_b);
    new_id bigint;
begin
    -- Advisory locks take a bigint key, so hash the uuid.
    perform pg_advisory_xact_lock(hashtextextended(lo::text, 0));
    perform pg_advisory_xact_lock(hashtextextended(hi::text, 0));

    if exists (
        select 1 from matches
        where (a_status = 'pending' or b_status = 'pending')
          and (user_a in (lo, hi) or user_b in (lo, hi))
          and created_at > now() - interval '2 minutes'
    ) then
        return null;
    end if;

    insert into matches (user_a, user_b, topic, score)
    values (lo, hi, p_topic, p_score)
    on conflict do nothing
    returning id into new_id;

    return new_id;
end;
$$;

-- Queries --------------------------------------------------------------------

-- PUSH (every 5 min)
-- insert into curr_task (user_id, task_summary, topic) values (:me, :summary, :topic)
-- on conflict (user_id) do update set
--     task_summary = excluded.task_summary, topic = excluded.topic, pushed_at = now();
-- insert into curr_task_history (user_id, task_summary, topic) values (:me, :summary, :topic);
-- delete from is_working_on_connections where user_id = :me;
-- insert into is_working_on_connections (connection_id, user_id)
--     select id, :me from connections where :me in (user_a, user_b) and topic = :topic;

-- JEV SEARCH (right after each push, for :me only). Prefilter candidates:
-- select t.user_id, t.task_summary, t.topic, u.bio
-- from curr_task t join users u on u.id = t.user_id
-- where t.user_id <> :me
--   and u.sharing_enabled
--   and t.pushed_at > now() - interval '11 minutes'                 -- skip dead laptops
--   and (u.age_group = 'under_18') = (:my_age_group = 'under_18')   -- minors only with minors
--   and not exists (select 1 from connections c                     -- already connected on topic
--                   where c.user_a = least(:me, t.user_id) and c.user_b = greatest(:me, t.user_id)
--                     and c.topic = t.topic)
--   and not exists (select 1 from matches m                         -- declined in last day
--                   where m.user_a = least(:me, t.user_id) and m.user_b = greatest(:me, t.user_id)
--                     and 'declined' in (m.a_status, m.b_status)
--                     and m.created_at > now() - interval '1 day');
-- Then Jev scores each candidate. For a good pair:
-- select create_match(:me, :other, :topic, :score);

-- QUIT / PAUSE
-- delete from curr_task                 where user_id = :me;
-- delete from is_working_on_connections where user_id = :me;

-- POLL (every 20 s) 1: match popups waiting for me (shows the other person's bio + task)
-- select m.id, m.topic, u.name as other_name, u.school, u.bio, t.task_summary
-- from matches m
-- join users u          on u.id = case when m.user_a = :me then m.user_b else m.user_a end
-- left join curr_task t on t.user_id = u.id
-- where ((m.user_a = :me and m.a_status = 'pending') or (m.user_b = :me and m.b_status = 'pending'))
--   and m.created_at > now() - interval '2 minutes';

-- POLL (every 20 s) 2: friends active on our topic (light up icon)
-- delete from is_working_on_connections where seen_at < now() - interval '11 minutes';
-- select c.id as connection_id, c.topic, u.name as friend_name
-- from is_working_on_connections w
-- join connections c on c.id = w.connection_id
-- join users u       on u.id = w.user_id
-- where :me in (c.user_a, c.user_b) and w.user_id <> :me;

-- ACCEPT: update matches set a_status = 'accepted' where id = :id and user_a = :me;  (b_status for user_b)
-- BOTH ACCEPTED -> start call:
-- select 1 from matches where id = :id and a_status = 'accepted' and b_status = 'accepted';
