-- Username + password sign-in (POST /auth/signup, /auth/login).
-- password_hash is "scrypt$<salt hex>$<hash hex>". Seeded dev users have
-- neither, so they can only be used through VITE_USER_ID.
-- schema.sql has these for fresh installs; safe to run twice.
alter table users add column if not exists username text unique;
alter table users add column if not exists password_hash text;
