ALTER TABLE tips DROP COLUMN currency_id;
ALTER TABLE tips RENAME TO tips_vrsc;

ALTER TABLE transactions DROP CONSTRAINT transactions_pkey;
ALTER TABLE transactions DROP COLUMN currency_id;
ALTER TABLE transactions RENAME TO transactions_vrsc;
ALTER TABLE transactions_vrsc ADD PRIMARY KEY (uuid, discord_id);

ALTER TABLE addresses DROP CONSTRAINT addresses_pkey;
ALTER TABLE addresses DROP COLUMN currency_id;
ALTER TABLE addresses ADD PRIMARY KEY (discord_id);

ALTER TABLE balances DROP CONSTRAINT balances_pkey;
ALTER TABLE balances DROP COLUMN currency_id;
ALTER TABLE balances ADD CONSTRAINT balance_vrsc_discord_id_key UNIQUE (discord_id);
ALTER TABLE balances RENAME TO balance_vrsc;

ALTER TABLE reactdrops DROP COLUMN currency_id;