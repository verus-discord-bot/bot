-- update tips table

ALTER TABLE tips_vrsc RENAME TO tips;
-- the DEFAULT is just for this migration, we will support other currencies in the future:
ALTER TABLE tips ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE tips ALTER COLUMN currency_id DROP DEFAULT;
ALTER TABLE tips RENAME COLUMN uuid TO id;

-- Start using postgres UUID type
ALTER TABLE tips ALTER COLUMN id TYPE uuid USING id::uuid;

-- change primary key to support multiple currencies
ALTER TABLE tips DROP CONSTRAINT tips_vrsc_pkey;
ALTER TABLE tips ADD PRIMARY KEY (id, currency_id, discord_id);

-- update transactions table
ALTER TABLE transactions_vrsc RENAME TO transactions;
ALTER TABLE transactions ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE transactions ALTER COLUMN currency_id DROP DEFAULT;
ALTER TABLE transactions RENAME COLUMN uuid TO id;
ALTER TABLE transactions ALTER COLUMN id TYPE uuid USING id::uuid;
ALTER TABLE transactions DROP CONSTRAINT transactions_vrsc_pkey;
ALTER TABLE transactions ADD PRIMARY KEY (id, currency_id, discord_id);

ALTER TABLE addresses ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE addresses ALTER COLUMN currency_id DROP DEFAULT;
ALTER TABLE addresses DROP CONSTRAINT addresses_pkey;
ALTER TABLE addresses ADD PRIMARY KEY (discord_id, currency_id);

ALTER TABLE balance_vrsc RENAME TO balances;
ALTER TABLE balances ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE balances ALTER COLUMN discord_id SET NOT NULL;
ALTER TABLE balances DROP CONSTRAINT balance_vrsc_discord_id_key;
ALTER TABLE balances ADD PRIMARY KEY (discord_id, currency_id);

BEGIN;

-- Insert missing discord_ids into discord_users
INSERT INTO discord_users (discord_id, created_at, updated_at)
SELECT DISTINCT b.discord_id, NOW(), NOW()
FROM balances b
LEFT JOIN discord_users du ON b.discord_id = du.discord_id
WHERE b.discord_id IS NOT NULL AND du.discord_id IS NULL
ON CONFLICT (discord_id) DO NOTHING;

-- Add the foreign key constraint
ALTER TABLE balances
ADD CONSTRAINT fk_discord_id
FOREIGN KEY (discord_id) REFERENCES discord_users(discord_id);

COMMIT;

ALTER TABLE balances ALTER COLUMN currency_id DROP DEFAULT;

-- Function to insert into discord_users when balance is inserted for a new user
CREATE OR REPLACE FUNCTION ensure_discord_user_exists()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO discord_users (discord_id, created_at, updated_at)
    VALUES (NEW.discord_id, NOW(), NOW())
    ON CONFLICT (discord_id) DO NOTHING;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger on balances
CREATE TRIGGER trigger_ensure_discord_user
BEFORE INSERT ON balances
FOR EACH ROW
WHEN (NEW.discord_id IS NOT NULL)
EXECUTE FUNCTION ensure_discord_user_exists();

ALTER TABLE reactdrops ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE reactdrops ALTER COLUMN currency_id DROP DEFAULT;

-- add enum type to transactions
-- migrate deposits and withdrawals, one time daemon RPC for every deposit and withdrawal
-- make mutations on balance, lock on SELECT when tipping

-- add amount to transactions (to speed up admin calls)