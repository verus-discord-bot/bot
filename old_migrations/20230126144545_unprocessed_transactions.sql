-- Add migration script here

CREATE TABLE
    public.unprocessed_transactions (
        txid TEXT NOT NULL PRIMARY KEY,
        status TEXT NOT NULL,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    ) TABLESPACE pg_default;

CREATE TRIGGER SET_UPDATED_TIMESTAMP 
	BEFORE
	UPDATE
	    ON public.unprocessed_transactions FOR EACH ROW
	EXECUTE
	    PROCEDURE trigger_set_timestamp();
