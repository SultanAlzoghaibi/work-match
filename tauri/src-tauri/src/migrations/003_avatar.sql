-- Profile picture: public URL in the backend's Supabase Storage bucket "avatars".
-- NULL = show the name's initial instead.

ALTER TABLE curr_user_profile ADD COLUMN avatar_url TEXT;
