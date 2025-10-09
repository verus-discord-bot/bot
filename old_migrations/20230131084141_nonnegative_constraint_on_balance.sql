-- Add migration script here
ALTER TABLE balance_vrsc
    ADD CONSTRAINT non_negative_balance check (balance >= 0);