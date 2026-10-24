-- Languages the user speaks, comma-separated ("English, Arabic"). Mirrors
-- backend users.languages (text[]), which the Filters tab's "shared language"
-- option uses.
ALTER TABLE curr_user_profile ADD COLUMN languages TEXT;
