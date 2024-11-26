-- -- update tips table

-- ALTER TABLE tips_vrsc RENAME TO tips;
-- ALTER TABLE tips ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
-- ALTER TABLE tips ALTER COLUMN currency_id DROP DEFAULT;
-- ALTER TABLE tips RENAME COLUMN uuid TO id;

-- -- Start using postgres UUID type
-- ALTER TABLE tips ALTER COLUMN id TYPE uuid USING id::uuid;

-- -- change primary key
-- ALTER TABLE tips DROP CONSTRAINT tips_vrsc_pkey;
-- ALTER TABLE tips ADD PRIMARY KEY (id, currency_id, discord_id);

-- -- update transactions table

-- ALTER TABLE transactions_vrsc RENAME TO transactions;
-- ALTER TABLE transactions ADD COLUMN currency_id TEXT NOT NULL DEFAULT 'i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV';
-- ALTER TABLE transactions ALTER COLUMN currency_id DROP DEFAULT;
-- ALTER TABLE transactions RENAME COLUMN uuid TO id;

-- -- Start using postgres UUID type
-- ALTER TABLE transactions ALTER COLUMN id TYPE uuid USING id::uuid;

-- -- change primary key
-- ALTER TABLE transactions DROP CONSTRAINT transactions_vrsc_pkey;
-- ALTER TABLE transactions ADD PRIMARY KEY (id, currency_id, discord_id);

-- -- move addresses to discord_users table
-- -- move balance to discord_users table
--     -- make balance default 0 (process_a_tip)
--     -- make mutations on balance, lock on SELECT when tipping
--     -- migrate all existing balances

-- -- use Decimal instead of bigint to store amounts (tips, balances and fees)

-- -- add amount to transactions
-- -- add enum type to transactions
-- -- migrate deposits and withdrawals, one time daemon RPC for every deposit and withdrawal
