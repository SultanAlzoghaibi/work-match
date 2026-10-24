-- Dev seed: the hard-coded users the Tauri app uses until real sign-up exists
-- (MOCK_USER_ID in tauri/src/api.js). ...0000 is instance 1 (yarn dev1),
-- ...0001 is instance 2 (yarn dev2). Safe to run more than once.
insert into users (id, token, name)
values ('00000000-0000-0000-0000-000000000000', 'dev-token', 'New User'),
       ('00000000-0000-0000-0000-000000000001', 'dev-token-2', 'New User 2')
on conflict (id) do nothing;
