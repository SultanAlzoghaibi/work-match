-- Demo data for local development. NOT run automatically: call
-- db::seeder::seed_defaults from setup in lib.rs to apply it.
-- Every statement is idempotent (INSERT OR IGNORE / fixed ids), so it is
-- safe to run more than once.

UPDATE curr_user_profile
SET name = 'Demo User',
    school = 'University of Calgary',
    bio = 'Grinding LeetCode before internship season',
    sharing_enabled = 1
WHERE id = 1 AND name IS NULL;

INSERT OR IGNORE INTO activity_log (id, source, app_name, bundle_id, title, topic, started_at, ended_at) VALUES
    (1, 'app',      'Google Chrome',      'com.google.Chrome',     NULL,                    'leetcode', datetime('now', '-3 hours'),  datetime('now', '-2 hours')),
    (2, 'app',      'Visual Studio Code', 'com.microsoft.VSCode',  NULL,                    'coding',   datetime('now', '-2 hours'),  datetime('now', '-1 hours')),
    (3, 'calendar', NULL,                 NULL,                    'CPSC 413 study block',  'algorithms', datetime('now', '-1 hours'), datetime('now', '-30 minutes')),
    (4, 'app',      'Figma',              'com.figma.Desktop',     NULL,                    'design',   datetime('now', '-20 minutes'), NULL);

INSERT OR IGNORE INTO settings (key, value) VALUES
    ('matching_paused', '0'),
    ('intro_cooldown_min', '30');
