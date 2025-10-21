-- we don't need the foreign key dependency
ALTER TABLE addresses DROP CONSTRAINT addresses_discord_id_fkey;
ALTER TABLE transactions DROP CONSTRAINT transactions_discord_id_fkey;

CREATE TABLE notifications (
    discord_id bigint PRIMARY KEY,
    loudness text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE blacklist (
    discord_id bigint PRIMARY KEY,
    blacklisted bool NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON notifications FOR EACH ROW EXECUTE FUNCTION trigger_set_timestamp();
CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON blacklist FOR EACH ROW EXECUTE FUNCTION trigger_set_timestamp();

-- migrate old settings
INSERT INTO notifications (discord_id, loudness)
SELECT discord_id, notifications
FROM discord_users
WHERE notifications IS NOT NULL;

INSERT INTO blacklist (discord_id, blacklisted)
SELECT discord_id, blacklisted
FROM discord_users
WHERE blacklisted = TRUE; 

DROP TABLE discord_users;
