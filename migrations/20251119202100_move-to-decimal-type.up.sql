-- left over lingering default
ALTER TABLE balances ALTER COLUMN currency_id DROP DEFAULT;

-- balances

ALTER TABLE balances ADD COLUMN amount_decimal DECIMAL(36,8);

UPDATE balances 
SET amount_decimal = balance::DECIMAL / 100000000.0 
WHERE balance IS NOT NULL;

ALTER TABLE balances ALTER COLUMN amount_decimal SET NOT NULL;
ALTER TABLE balances DROP COLUMN balance;
ALTER TABLE balances RENAME COLUMN amount_decimal TO amount;


-- transactions fee - amount - tx_fee
ALTER TABLE transactions ADD COLUMN fee_decimal DECIMAL(36,8);
ALTER TABLE transactions ADD COLUMN amount_decimal DECIMAL(36,8);
ALTER TABLE transactions ADD COLUMN tx_fee_decimal DECIMAL(36,8);

UPDATE transactions 
SET fee_decimal = fee::DECIMAL / 100000000.0 
WHERE fee IS NOT NULL;

UPDATE transactions 
SET amount_decimal = amount::DECIMAL / 100000000.0 
WHERE amount IS NOT NULL;

UPDATE transactions 
SET tx_fee_decimal = tx_fee::DECIMAL / 100000000.0 
WHERE tx_fee IS NOT NULL;

ALTER TABLE transactions ALTER COLUMN fee_decimal SET NOT NULL;
ALTER TABLE transactions ALTER COLUMN amount_decimal SET NOT NULL;
ALTER TABLE transactions ALTER COLUMN tx_fee_decimal SET NOT NULL;
ALTER TABLE transactions DROP COLUMN fee;
ALTER TABLE transactions DROP COLUMN amount;
ALTER TABLE transactions DROP COLUMN tx_fee;
ALTER TABLE transactions RENAME COLUMN fee_decimal TO fee;
ALTER TABLE transactions RENAME COLUMN amount_decimal TO amount;
ALTER TABLE transactions RENAME COLUMN tx_fee_decimal TO tx_fee;

-- opids
ALTER TABLE opids ADD COLUMN amount_decimal DECIMAL(36,8);

UPDATE opids 
SET amount_decimal = amount::DECIMAL / 100000000.0 
WHERE amount IS NOT NULL;

ALTER TABLE opids ALTER COLUMN amount_decimal SET NOT NULL;
ALTER TABLE opids DROP COLUMN amount;
ALTER TABLE opids RENAME COLUMN amount_decimal TO amount;

-- reactdrops
ALTER TABLE reactdrops ADD COLUMN amount_decimal DECIMAL(36,8);

UPDATE reactdrops 
SET amount_decimal = amount::DECIMAL / 100000000.0 
WHERE amount IS NOT NULL;

ALTER TABLE reactdrops ALTER COLUMN amount_decimal SET NOT NULL;
ALTER TABLE reactdrops DROP COLUMN amount;
ALTER TABLE reactdrops RENAME COLUMN amount_decimal TO amount;

-- tips
ALTER TABLE tips ADD COLUMN amount_decimal DECIMAL(36,8);

UPDATE tips 
SET amount_decimal = amount::DECIMAL / 100000000.0 
WHERE amount IS NOT NULL;

ALTER TABLE tips ALTER COLUMN amount_decimal SET NOT NULL;
ALTER TABLE tips DROP COLUMN amount;
ALTER TABLE tips RENAME COLUMN amount_decimal TO amount;
