-- Add migration script here
-- There is a relation between discord_users and this table: Every transaction a user does, ends up in this table.
-- This table should always exist, even if discord_users gets deleted somehow in the future.
CREATE TABLE public.transactions_vrsc
(
    discord_id bigint NOT NULL,
    transaction_id character varying(64) COLLATE pg_catalog."default" NOT NULL,
    transaction_action text COLLATE pg_catalog."default" NOT NULL,
    CONSTRAINT transactions_pkey PRIMARY KEY (discord_id, transaction_id),
    CONSTRAINT transactions_discord_id_fkey FOREIGN KEY (discord_id)
        REFERENCES public.discord_users (discord_id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE NO ACTION
)

TABLESPACE pg_default;