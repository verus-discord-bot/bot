-- Add migration script here
CREATE TABLE
    public.reactdrops (
        channel_id bigint NOT NULL,
        message_id bigint NOT NULL,
        finish_time TIMESTAMPTZ NOT NULL,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

        PRIMARY KEY (channel_id, message_id)
    ) TABLESPACE pg_default;