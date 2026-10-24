-- work-match on Supabase (Postgres). Port of backend/schema.sql plus the AI
-- pipeline from docs/SPEC.md.
--
-- Who writes what:
--   Mac app (signed-in user, RLS):  own profile, own activity, respond_to_offer(), answer_friend()
--   Python matcher (service role):  offers, match_log (via create_offer())
--   Triggers:                       connections + cowork_sessions when both accept
--
-- Every pair (user_a, user_b) is stored with user_a < user_b.

-- Profiles --------------------------------------------------------------------

create table public.profiles (
    id              uuid primary key references auth.users (id) on delete cascade,
    name            text not null,
    school          text,
    bio             text,                       -- one line, matching context
    field           text,                       -- e.g. "computer science"
    discord_handle  text,
    sharing_enabled boolean not null default false,
    created_at      timestamptz not null default now()
);

-- Latest 5-minute summary per user. Doubles as presence: a user counts as
-- online while updated_at is recent. Only the owner and the matcher can read it.
create table public.activity (
    user_id       uuid primary key references public.profiles (id) on delete cascade,
    summary       text not null,                -- 2-4 sentences from the local model
    topic         text not null,                -- e.g. "graph algorithms"
    subtopics     text[] not null default '{}',
    context       text,                         -- e.g. "CS170 homework"
    activity_type text,                         -- e.g. "coding / debugging"
    focus         text not null default 'medium' check (focus in ('low', 'medium', 'high')),
    stuck         boolean not null default false,
    updated_at    timestamptz not null default now()
);

-- Connections -----------------------------------------------------------------

-- Starts as a trial after both accept an offer, becomes friends if both opt in
-- after the first session, otherwise ended.
create table public.connections (
    id                bigint generated always as identity primary key,
    user_a            uuid not null references public.profiles (id) on delete cascade,
    user_b            uuid not null references public.profiles (id) on delete cascade,
    status            text not null default 'trial' check (status in ('trial', 'friends', 'ended')),
    topic             text not null,            -- what they matched on
    a_wants_friend    boolean,                  -- null until answered
    b_wants_friend    boolean,
    created_at        timestamptz not null default now(),
    became_friends_at timestamptz,
    ended_at          timestamptz,
    check (user_a < user_b)
);

create unique index uq_connections_live_pair
    on public.connections (user_a, user_b)
    where status in ('trial', 'friends');

-- Offers ----------------------------------------------------------------------

-- A proposed match, shown as the glass popup. Both people can read the row, so
-- it only holds what both may see; the matcher's inputs go in match_log.
create table public.offers (
    id            bigint generated always as identity primary key,
    user_a        uuid not null references public.profiles (id) on delete cascade,
    user_b        uuid not null references public.profiles (id) on delete cascade,
    activity      text not null,                -- shared topic shown to both
    score         real not null check (score between 0 and 1),   -- Jev P(good match)
    intro_a       text not null,                -- popup text shown to user_a
    intro_b       text not null,                -- ... and to user_b
    a_status      text not null default 'pending' check (a_status in ('pending', 'accepted', 'declined')),
    b_status      text not null default 'pending' check (b_status in ('pending', 'accepted', 'declined')),
    created_at    timestamptz not null default now(),
    expires_at    timestamptz not null default now() + interval '2 minutes',
    connection_id bigint references public.connections (id) on delete set null,
    check (user_a < user_b)
);

create index idx_offers_user_a on public.offers (user_a, created_at);
create index idx_offers_user_b on public.offers (user_b, created_at);
create index idx_offers_created_at on public.offers (created_at);

-- What each match decision was based on. Matcher-only (no RLS policies), so
-- one person's summary is never readable by the other. Past rows are fed back
-- to Jev as examples.
create table public.match_log (
    offer_id       bigint primary key references public.offers (id) on delete cascade,
    summary_a      text not null,
    summary_b      text not null,
    interrupt_ok   real check (interrupt_ok between 0 and 1),   -- min of both
    decision_model text not null,               -- e.g. "typesafe/jev-1.14", "fallback:topic"
    intro_model    text not null,               -- e.g. Muse model id, "fallback:template"
    created_at     timestamptz not null default now()
);

-- Sessions and messages -------------------------------------------------------

create table public.cowork_sessions (
    id            bigint generated always as identity primary key,
    connection_id bigint not null references public.connections (id) on delete cascade,
    started_at    timestamptz not null default now(),
    duration_min  integer not null default 25 check (duration_min > 0),
    call_url      text,                         -- Discord invite/call link
    ended_at      timestamptz,                  -- null = active
    end_reason    text check (end_reason in ('timer', 'user_left', 'disconnected')),
    goal_a        text,
    goal_b        text,
    outcome_a     text,
    outcome_b     text
);

create unique index uq_sessions_one_active
    on public.cowork_sessions (connection_id)
    where ended_at is null;

create table public.messages (
    id            bigint generated always as identity primary key,
    connection_id bigint not null references public.connections (id) on delete cascade,
    session_id    bigint references public.cowork_sessions (id) on delete set null,
    sender_id     uuid not null references public.profiles (id) on delete cascade,
    text          text not null,
    sent_at       timestamptz not null default now()
);

create index idx_messages_connection on public.messages (connection_id, sent_at);

-- Helpers -----------------------------------------------------------------------

-- In an active session, or holding an offer that hasn't been answered or expired.
create function public.is_busy(uid uuid)
returns boolean
language sql stable security definer set search_path = public
as $$
    select exists (
        select 1 from cowork_sessions s
        join connections c on c.id = s.connection_id
        where s.ended_at is null and uid in (c.user_a, c.user_b)
    ) or exists (
        select 1 from offers o
        where uid in (o.user_a, o.user_b)
          and o.expires_at > now()
          and o.connection_id is null
          and 'declined' not in (o.a_status, o.b_status)
    );
$$;

-- Users the matcher should try to match: sharing on, summary from the last
-- 10 minutes, not busy. Newest first.
create function public.eligible_users(max_age interval default interval '10 minutes')
returns setof public.activity
language sql stable security definer set search_path = public
as $$
    select a.* from activity a
    join profiles p on p.id = a.user_id
    where p.sharing_enabled
      and a.updated_at > now() - max_age
      and not is_busy(a.user_id)
    order by a.updated_at desc;
$$;

-- Candidates for one user: eligible, not declined by/with them in the last
-- day, no live connection already (friends get "working now" pings instead).
create function public.match_candidates(me uuid, max_age interval default interval '10 minutes', lim integer default 10)
returns table (
    user_id uuid, name text, school text, bio text, field text,
    summary text, topic text, subtopics text[], context text,
    activity_type text, focus text, stuck boolean
)
language sql stable security definer set search_path = public
as $$
    select a.user_id, p.name, p.school, p.bio, p.field,
           a.summary, a.topic, a.subtopics, a.context,
           a.activity_type, a.focus, a.stuck
    from eligible_users(max_age) a
    join profiles p on p.id = a.user_id
    where a.user_id <> me
      and not exists (
          select 1 from offers o
          where o.user_a = least(me, a.user_id) and o.user_b = greatest(me, a.user_id)
            and 'declined' in (o.a_status, o.b_status)
            and o.created_at > now() - interval '1 day'
      )
      and not exists (
          select 1 from connections c
          where c.user_a = least(me, a.user_id) and c.user_b = greatest(me, a.user_id)
            and c.status in ('trial', 'friends')
      )
    limit lim;
$$;

-- Insert an offer and its log row. Re-checks that neither person became busy
-- since the matcher read them; returns null if so.
create function public.create_offer(
    p_one uuid, p_two uuid, p_activity text, p_score real,
    p_intro_one text, p_intro_two text,
    p_summary_one text, p_summary_two text,
    p_interrupt_ok real, p_decision_model text, p_intro_model text
)
returns bigint
language plpgsql security definer set search_path = public
as $$
declare
    swap boolean := p_one > p_two;
    new_id bigint;
begin
    if p_one = p_two then
        raise exception 'cannot match a user with themselves';
    end if;
    -- Serialize offer creation for these two users.
    perform pg_advisory_xact_lock(hashtext(least(p_one, p_two)::text));
    perform pg_advisory_xact_lock(hashtext(greatest(p_one, p_two)::text));
    if is_busy(p_one) or is_busy(p_two) then
        return null;
    end if;

    insert into offers (user_a, user_b, activity, score, intro_a, intro_b)
    values (
        least(p_one, p_two), greatest(p_one, p_two), p_activity, p_score,
        case when swap then p_intro_two else p_intro_one end,
        case when swap then p_intro_one else p_intro_two end
    )
    returning id into new_id;

    insert into match_log (offer_id, summary_a, summary_b, interrupt_ok, decision_model, intro_model)
    values (
        new_id,
        case when swap then p_summary_two else p_summary_one end,
        case when swap then p_summary_one else p_summary_two end,
        p_interrupt_ok, p_decision_model, p_intro_model
    );
    return new_id;
end;
$$;

-- Past decided offers and how they turned out, for Jev examples.
create function public.recent_outcomes(lim integer default 5)
returns table (summary_a text, summary_b text, a_status text, b_status text, connection_status text)
language sql stable security definer set search_path = public
as $$
    select l.summary_a, l.summary_b, o.a_status, o.b_status, c.status
    from offers o
    join match_log l on l.offer_id = o.id
    left join connections c on c.id = o.connection_id
    where o.a_status <> 'pending' and o.b_status <> 'pending'
    order by o.created_at desc
    limit lim;
$$;

-- Client actions --------------------------------------------------------------

-- Accept or decline your side of an offer. When both accept, the connection
-- and the first 25-minute session are created.
create function public.respond_to_offer(p_offer_id bigint, p_accept boolean)
returns public.offers
language plpgsql security definer set search_path = public
as $$
declare
    me uuid := auth.uid();
    o offers;
    new_connection bigint;
begin
    select * into o from offers where id = p_offer_id for update;
    if not found or me not in (o.user_a, o.user_b) then
        raise exception 'offer not found';
    end if;
    if o.expires_at <= now() then
        raise exception 'offer expired';
    end if;
    if (me = o.user_a and o.a_status <> 'pending') or (me = o.user_b and o.b_status <> 'pending') then
        raise exception 'already answered';
    end if;

    if me = o.user_a then
        update offers set a_status = case when p_accept then 'accepted' else 'declined' end
        where id = o.id returning * into o;
    else
        update offers set b_status = case when p_accept then 'accepted' else 'declined' end
        where id = o.id returning * into o;
    end if;

    if o.a_status = 'accepted' and o.b_status = 'accepted' then
        insert into connections (user_a, user_b, topic)
        values (o.user_a, o.user_b, o.activity)
        on conflict (user_a, user_b) where status in ('trial', 'friends') do nothing
        returning id into new_connection;
        if new_connection is null then
            select id into new_connection from connections
            where user_a = o.user_a and user_b = o.user_b and status in ('trial', 'friends');
        end if;
        insert into cowork_sessions (connection_id) values (new_connection);
        update offers set connection_id = new_connection where id = o.id returning * into o;
    end if;
    return o;
end;
$$;

-- After the first session: "add as friend?" Both yes = friends, either no = ended.
create function public.answer_friend(p_connection_id bigint, p_wants boolean)
returns public.connections
language plpgsql security definer set search_path = public
as $$
declare
    me uuid := auth.uid();
    c connections;
begin
    select * into c from connections where id = p_connection_id for update;
    if not found or me not in (c.user_a, c.user_b) then
        raise exception 'connection not found';
    end if;
    if c.status <> 'trial' then
        raise exception 'connection is not a trial';
    end if;

    if me = c.user_a then
        update connections set a_wants_friend = p_wants where id = c.id returning * into c;
    else
        update connections set b_wants_friend = p_wants where id = c.id returning * into c;
    end if;

    if c.a_wants_friend is false or c.b_wants_friend is false then
        update connections set status = 'ended', ended_at = now() where id = c.id returning * into c;
    elsif c.a_wants_friend and c.b_wants_friend then
        update connections set status = 'friends', became_friends_at = now() where id = c.id returning * into c;
    end if;
    return c;
end;
$$;

-- Permissions -----------------------------------------------------------------

alter table public.profiles        enable row level security;
alter table public.activity        enable row level security;
alter table public.connections     enable row level security;
alter table public.offers          enable row level security;
alter table public.match_log       enable row level security;
alter table public.cowork_sessions enable row level security;
alter table public.messages        enable row level security;

-- Your own profile, plus people you have an offer or connection with.
create policy "profiles: read own and matched" on public.profiles for select to authenticated
using (
    profiles.id = auth.uid()
    or exists (select 1 from offers o where (o.user_a, o.user_b) in ((auth.uid(), profiles.id), (profiles.id, auth.uid())))
    or exists (select 1 from connections c where (c.user_a, c.user_b) in ((auth.uid(), profiles.id), (profiles.id, auth.uid())))
);
create policy "profiles: insert own" on public.profiles for insert to authenticated
with check (id = auth.uid());
create policy "profiles: update own" on public.profiles for update to authenticated
using (id = auth.uid()) with check (id = auth.uid());

-- Activity is private to its owner (the matcher uses the service role).
create policy "activity: read own" on public.activity for select to authenticated
using (user_id = auth.uid());
create policy "activity: insert own" on public.activity for insert to authenticated
with check (user_id = auth.uid());
create policy "activity: update own" on public.activity for update to authenticated
using (user_id = auth.uid()) with check (user_id = auth.uid());
create policy "activity: delete own" on public.activity for delete to authenticated
using (user_id = auth.uid());

-- Offers and connections: read your own; change them only through the functions above.
create policy "offers: read own" on public.offers for select to authenticated
using (auth.uid() in (user_a, user_b));
create policy "connections: read own" on public.connections for select to authenticated
using (auth.uid() in (user_a, user_b));

create policy "sessions: read own" on public.cowork_sessions for select to authenticated
using (exists (select 1 from connections c where c.id = connection_id and auth.uid() in (c.user_a, c.user_b)));

create policy "messages: read own" on public.messages for select to authenticated
using (exists (select 1 from connections c where c.id = connection_id and auth.uid() in (c.user_a, c.user_b)));
create policy "messages: send in own connection" on public.messages for insert to authenticated
with check (
    sender_id = auth.uid()
    and exists (select 1 from connections c where c.id = connection_id and auth.uid() in (c.user_a, c.user_b))
);

-- Matcher-only helpers.
revoke execute on function public.is_busy(uuid) from public, anon, authenticated;
revoke execute on function public.eligible_users(interval) from public, anon, authenticated;
revoke execute on function public.match_candidates(uuid, interval, integer) from public, anon, authenticated;
revoke execute on function public.create_offer(uuid, uuid, text, real, text, text, text, text, real, text, text) from public, anon, authenticated;
revoke execute on function public.recent_outcomes(integer) from public, anon, authenticated;
grant execute on function public.is_busy(uuid) to service_role;
grant execute on function public.eligible_users(interval) to service_role;
grant execute on function public.match_candidates(uuid, interval, integer) to service_role;
grant execute on function public.create_offer(uuid, uuid, text, real, text, text, text, text, real, text, text) to service_role;
grant execute on function public.recent_outcomes(integer) to service_role;

-- Client actions: signed-in users only.
revoke execute on function public.respond_to_offer(bigint, boolean) from public, anon;
revoke execute on function public.answer_friend(bigint, boolean) from public, anon;
grant execute on function public.respond_to_offer(bigint, boolean) to authenticated;
grant execute on function public.answer_friend(bigint, boolean) to authenticated;

-- Live updates to the Mac app (RLS still decides who receives which rows).
alter publication supabase_realtime add table public.offers, public.connections, public.cowork_sessions, public.messages;
