-- Add migration script here
DROP TABLE balance_vrsc;

CREATE TABLE public.balance_vrsc
(
    discord_id bigint REFERENCES discord_users(discord_id) UNIQUE,
    balance bigint DEFAULT 0
)

TABLESPACE pg_default;