-- Add migration script here
ALTER TABLE public.tips_vrsc RENAME COLUMN tip_action TO kind;
