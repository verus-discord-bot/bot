-- Add migration script here
ALTER TABLE public.transactions_vrsc DROP CONSTRAINT transactions_pkey;
ALTER TABLE public.transactions_vrsc ADD COLUMN uuid TEXT NOT NULL;
ALTER TABLE public.transactions_vrsc ADD CONSTRAINT transactions_vrsc_pkey PRIMARY KEY (uuid, discord_id);
