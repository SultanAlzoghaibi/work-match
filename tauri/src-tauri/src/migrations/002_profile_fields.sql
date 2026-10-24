-- Profile fields that mirror backend users (age_group, country, discord_handle),
-- so the edit form can fill them before the backend exists.

ALTER TABLE curr_user_profile ADD COLUMN age_group TEXT
    CHECK (age_group IN ('under_18', '18_24', '25_34', '35_44', '45_plus'));
ALTER TABLE curr_user_profile ADD COLUMN country TEXT;          -- ISO code, e.g. 'CA'
ALTER TABLE curr_user_profile ADD COLUMN discord_handle TEXT;
