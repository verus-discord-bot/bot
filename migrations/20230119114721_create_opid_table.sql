-- Add migration script here

CREATE TABLE
    public.opids (
        opid TEXT NOT NULL PRIMARY KEY,
        status TEXT NOT NULL,
        creation_time bigint NOT NULL,
        result TEXT,
        address TEXT NOT NULL,
        amount BIGINT NOT NULL,
        currency TEXT NOT NULL,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    ) TABLESPACE pg_default;

CREATE TRIGGER SET_UPDATED_TIMESTAMP 
	BEFORE
	UPDATE
	    ON public.opids FOR EACH ROW
	EXECUTE
	    PROCEDURE trigger_set_timestamp();
