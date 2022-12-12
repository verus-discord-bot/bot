-- Add migration script here
CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.balance_vrsc FOR EACH ROW EXECUTE PROCEDURE trigger_set_timestamp();
