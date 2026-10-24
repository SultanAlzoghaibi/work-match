-- Real sign-in (POST /auth/signup, /auth/login). user_id is the backend
-- users.id (uuid); username is kept after logout to prefill the login form.
ALTER TABLE curr_user_profile ADD COLUMN user_id TEXT;
ALTER TABLE curr_user_profile ADD COLUMN username TEXT;
