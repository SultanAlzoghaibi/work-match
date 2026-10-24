-- Jev search logs (jev_runs, jev_comparisons). schema.sql has them for fresh
-- installs; safe to run twice.
-- 6. Observability for the Jev search: one jev_runs row per match search (every
--    push), one jev_comparisons row per candidate Jev scored in it. Written by
--    matching.find_match; logging failures never block a match.
create table if not exists jev_runs (
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
create index if not exists idx_jev_runs_user on jev_runs (user_id, created_at desc);
create index if not exists idx_jev_runs_created on jev_runs (created_at desc);

create table if not exists jev_comparisons (
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
create index if not exists idx_jev_comparisons_run on jev_comparisons (run_id);
create index if not exists idx_jev_comparisons_candidate on jev_comparisons (candidate_user_id);

alter table jev_runs        enable row level security;
alter table jev_comparisons enable row level security;
