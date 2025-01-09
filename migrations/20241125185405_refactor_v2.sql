-- update tips table

ALTER TABLE tips_vrsc RENAME TO tips;
ALTER TABLE tips ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE tips ALTER COLUMN currency_id DROP DEFAULT;
ALTER TABLE tips RENAME COLUMN uuid TO id;

-- Start using postgres UUID type
ALTER TABLE tips ALTER COLUMN id TYPE uuid USING id::uuid;

-- change primary key
ALTER TABLE tips DROP CONSTRAINT tips_vrsc_pkey;
ALTER TABLE tips ADD PRIMARY KEY (id, currency_id, discord_id);

-- update transactions table

ALTER TABLE transactions_vrsc RENAME TO transactions;
ALTER TABLE transactions ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE transactions ALTER COLUMN currency_id DROP DEFAULT;
ALTER TABLE transactions RENAME COLUMN uuid TO id;

-- Start using postgres UUID type
ALTER TABLE transactions ALTER COLUMN id TYPE uuid USING id::uuid;

-- change primary key
ALTER TABLE transactions DROP CONSTRAINT transactions_vrsc_pkey;
ALTER TABLE transactions ADD PRIMARY KEY (id, currency_id, discord_id);

ALTER TABLE addresses ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE addresses ALTER COLUMN currency_id DROP DEFAULT;

-- change primary key
ALTER TABLE addresses DROP CONSTRAINT addresses_pkey;
ALTER TABLE addresses ADD PRIMARY KEY (discord_id, currency_id);

ALTER TABLE balance_vrsc RENAME TO balance;
ALTER TABLE balance ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
ALTER TABLE balance ALTER COLUMN currency_id DROP DEFAULT;

-- change primary key
ALTER TABLE balance DROP CONSTRAINT balance_vrsc_discord_id_key;
ALTER TABLE balance ADD PRIMARY KEY (discord_id, currency_id);
ALTER TABLE balance ADD CONSTRAINT fk_discord_id FOREIGN KEY (discord_id) REFERENCES discord_users (discord_id);
-- note: addresses already has a foreign key

-- use Decimal instead of bigint to store amounts (tips, balances and fees)
-- TODO test this properly
ALTER TABLE tips ALTER COLUMN amount TYPE numeric USING amount::NUMERIC / 100000000;
ALTER TABLE transactions ALTER COLUMN fee TYPE numeric USING fee::NUMERIC / 100000000;
ALTER TABLE balance ALTER COLUMN balance TYPE numeric USING balance::NUMERIC / 100000000;
ALTER TABLE opids ALTER COLUMN amount TYPE numeric USING amount::NUMERIC / 100000000;
ALTER TABLE reactdrops ALTER COLUMN amount TYPE numeric USING amount::NUMERIC / 100000000;



-- add amount to transactions (to speed up admin calls)
-- add enum type to transactions
-- migrate deposits and withdrawals, one time daemon RPC for every deposit and withdrawal
-- make mutations on balance, lock on SELECT when tipping
