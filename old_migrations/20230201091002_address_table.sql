-- Add migration script here
CREATE TABLE public.addresses
(
    discord_id bigint NOT NULL,
    address TEXT NOT NULL,
    CONSTRAINT addresses_pkey PRIMARY KEY (discord_id),
    CONSTRAINT addresses_discord_id_fkey FOREIGN KEY (discord_id)
        REFERENCES public.discord_users (discord_id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE NO ACTION
);

