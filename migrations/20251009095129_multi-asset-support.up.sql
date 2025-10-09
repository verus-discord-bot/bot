ALTER TABLE tips_vrsc RENAME TO tips;
-- the DEFAULT is just for this migration, we will support other currencies in the future:
ALTER TABLE tips ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE tips ALTER COLUMN currency_id DROP DEFAULT;

ALTER TABLE transactions_vrsc RENAME TO transactions;
ALTER TABLE transactions ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE transactions ALTER COLUMN currency_id DROP DEFAULT;
ALTER TABLE transactions DROP CONSTRAINT transactions_vrsc_pkey;
ALTER TABLE transactions ADD PRIMARY KEY (uuid, currency_id, discord_id);

ALTER TABLE addresses ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE addresses ALTER COLUMN currency_id DROP DEFAULT;
ALTER TABLE addresses DROP CONSTRAINT addresses_pkey;
ALTER TABLE addresses ADD PRIMARY KEY (discord_id, currency_id);

ALTER TABLE balance_vrsc RENAME TO balances;
ALTER TABLE balances ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE balances ALTER COLUMN discord_id SET NOT NULL;
ALTER TABLE balances DROP CONSTRAINT balance_vrsc_discord_id_key;
ALTER TABLE balances ADD PRIMARY KEY (discord_id, currency_id);

ALTER TABLE reactdrops ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE reactdrops ALTER COLUMN currency_id DROP DEFAULT;
