-- Add migration script here
CREATE TABLE public.balance_vrsc
(
    discord_id bigint NOT NULL,
    balance bigint DEFAULT 0,
    CONSTRAINT transactions_discord_id_fkey FOREIGN KEY (discord_id)
        REFERENCES public.discord_users (discord_id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE NO ACTION
)

TABLESPACE pg_default;