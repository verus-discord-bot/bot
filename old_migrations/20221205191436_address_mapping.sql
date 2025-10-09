-- Add migration script here
CREATE TABLE public.discord_users
(
    discord_id bigint NOT NULL,
    vrsc_address character varying(52) COLLATE pg_catalog."default" NOT NULL,
    PRIMARY KEY (discord_id)
)

TABLESPACE pg_default;