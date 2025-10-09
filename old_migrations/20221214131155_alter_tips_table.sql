-- Add migration script here
DROP TABLE tips_vrsc;

CREATE TABLE
    public.tips_vrsc (
        uuid TEXT NOT NULL,
        discord_id bigint NOT NULL,
        -- send / recv
        tip_action text NOT NULL,
        amount bigint NOT NULL,
        counterparty text NOT NULL,
        -- destination TEXT,
        -- source TEXT,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        PRIMARY KEY (uuid, discord_id),
        CONSTRAINT tips_discord_id_fkey FOREIGN KEY (discord_id) REFERENCES public.discord_users (discord_id) MATCH SIMPLE ON UPDATE NO ACTION ON DELETE NO ACTION
        
    ) TABLESPACE pg_default;

CREATE TRIGGER SET_UPDATED_TIMESTAMP 
	BEFORE
	UPDATE
	    ON public.tips_vrsc FOR EACH ROW
	EXECUTE
	    PROCEDURE trigger_set_timestamp();
