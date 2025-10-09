-- Add migration script here

ALTER TABLE public.discord_users ADD COLUMN blacklisted BOOLEAN DEFAULT false;