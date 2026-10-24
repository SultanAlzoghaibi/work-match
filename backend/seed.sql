-- Dev seed: the hard-coded user the Tauri app uses until real sign-up exists
-- (MOCK_USER_ID in tauri/src/api.js). Safe to run more than once.
insert into users (id, token, name)
values ('00000000-0000-0000-0000-000000000000', 'dev-token', 'New User')
on conflict (id) do nothing;
