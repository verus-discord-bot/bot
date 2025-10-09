-- Add migration script here
ALTER TABLE public.transactions_vrsc ADD COLUMN fee BIGINT;
