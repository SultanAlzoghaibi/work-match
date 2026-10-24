-- Add the "keep in touch?" answers to an existing database (schema.sql already
-- has them for fresh installs). Safe to run more than once.
alter table matches add column if not exists a_keep boolean;
alter table matches add column if not exists b_keep boolean;
