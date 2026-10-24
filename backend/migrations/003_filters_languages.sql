-- Match filters (Filters tab in the app) and spoken languages. schema.sql has
-- these for fresh installs; run this on an existing database. Safe to run twice.
alter table users add column if not exists languages text[] not null default '{}';

create table if not exists user_filters (
    user_id         uuid        primary key references users(id) on delete cascade,
    age_groups      text[]      not null default '{}',           -- allowed age groups; empty = any
    countries       char(2)[]   not null default '{}',           -- allowed countries (ISO); empty = anywhere
    shared_language boolean     not null default false,          -- only people who share a language with me
    updated_at      timestamptz not null default now()
);

alter table user_filters enable row level security;
