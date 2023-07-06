-- Add migration script here
ALTER TABLE reactdrops DROP COLUMN author;
ALTER TABLE reactdrops ADD COLUMN author bigint NOT NULL;