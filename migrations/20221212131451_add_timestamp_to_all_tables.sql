-- Add migration script here
ALTER TABLE public.transactions_vrsc ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE public.transactions_vrsc ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.transactions_vrsc FOR EACH ROW EXECUTE PROCEDURE trigger_set_timestamp();

ALTER TABLE public.discord_users ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE public.discord_users ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.discord_users FOR EACH ROW EXECUTE PROCEDURE trigger_set_timestamp();

