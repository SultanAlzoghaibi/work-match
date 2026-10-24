-- Local demo data (supabase db reset). Three users, password "password":
-- Alex and Sam are doing related work, Jo is not. Never run against production.

insert into auth.users (
    instance_id, id, aud, role, email, encrypted_password, email_confirmed_at,
    raw_app_meta_data, raw_user_meta_data, created_at, updated_at,
    confirmation_token, email_change, email_change_token_new, recovery_token
)
select '00000000-0000-0000-0000-000000000000', u.id, 'authenticated', 'authenticated', u.email,
       extensions.crypt('password', extensions.gen_salt('bf')), now(),
       '{"provider":"email","providers":["email"]}', '{}', now(), now(), '', '', '', ''
from (values
    ('11111111-1111-1111-1111-111111111111'::uuid, 'alex@example.com'),
    ('22222222-2222-2222-2222-222222222222'::uuid, 'sam@example.com'),
    ('33333333-3333-3333-3333-333333333333'::uuid, 'jo@example.com')
) as u (id, email);

insert into auth.identities (id, user_id, provider_id, identity_data, provider, last_sign_in_at, created_at, updated_at)
select gen_random_uuid(), id, id::text, jsonb_build_object('sub', id::text, 'email', email), 'email', now(), now(), now()
from auth.users
where email in ('alex@example.com', 'sam@example.com', 'jo@example.com');

insert into public.profiles (id, name, school, bio, field, discord_handle, sharing_enabled) values
    ('11111111-1111-1111-1111-111111111111', 'Alex', 'UC Berkeley', 'CS junior, grinding algorithms', 'computer science', 'alex#0001', true),
    ('22222222-2222-2222-2222-222222222222', 'Sam',  'UC Berkeley', 'Prepping for CS170 midterm',     'computer science', 'sam#0002',  true),
    ('33333333-3333-3333-3333-333333333333', 'Jo',   'CCA',         'Product designer',              'design',           'jo#0003',   true);

insert into public.activity (user_id, summary, topic, subtopics, context, activity_type, focus, stuck) values
    ('11111111-1111-1111-1111-111111111111',
     'Implementing Dijkstra''s algorithm in Python for CS170 Homework 4. Started with a priority-queue version and is now debugging wrong distances on a test case.',
     'graph algorithms', '{dijkstra,priority queues,python}', 'CS170 homework', 'coding / debugging', 'high', true),
    ('22222222-2222-2222-2222-222222222222',
     'Watching a lecture on shortest-path algorithms and taking notes. Comparing Dijkstra with Bellman-Ford for the CS170 midterm.',
     'graph algorithms', '{shortest paths,bellman-ford,dijkstra}', 'CS170 midterm prep', 'studying', 'medium', false),
    ('33333333-3333-3333-3333-333333333333',
     'Designing an onboarding flow for a mobile app in Figma, iterating on the sign-up screens.',
     'mobile app design', '{onboarding,figma}', null, 'designing', 'medium', false);
