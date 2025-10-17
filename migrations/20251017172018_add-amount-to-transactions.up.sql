ALTER TABLE transactions ADD COLUMN amount bigint;
ALTER TABLE transactions ADD COLUMN tx_fee bigint;
ALTER TABLE transactions ADD COLUMN address TEXT;
