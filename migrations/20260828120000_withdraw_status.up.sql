ALTER TABLE transactions
    ADD COLUMN status TEXT NOT NULL DEFAULT 'sent';
