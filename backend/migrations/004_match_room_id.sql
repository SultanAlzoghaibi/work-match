-- Jitsi room for each match, set by POST .../respond when the second person
-- accepts. The apps see it in their poll and join https://meet.jit.si/<room_id>.
-- schema.sql has it for fresh installs; safe to run twice.
alter table matches add column if not exists room_id text;
