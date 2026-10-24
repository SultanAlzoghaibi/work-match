-- History of every push (curr_task only keeps the latest). schema.sql has it
-- for fresh installs; run this on an existing database. Safe to run twice.
create table if not exists curr_task_history (
    id           bigint generated always as identity primary key,
    user_id      uuid        not null references users(id) on delete cascade,
    task_summary text        not null,
    topic        text,
    pushed_at    timestamptz not null default now()
);

create index if not exists idx_curr_task_history_user on curr_task_history (user_id, pushed_at desc);

alter table curr_task_history enable row level security;
