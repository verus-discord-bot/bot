-- Add migration script here
INSERT INTO addresses 
SELECT discord_id, vrsc_address
FROM discord_users;