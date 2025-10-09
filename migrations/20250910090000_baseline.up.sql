--
-- PostgreSQL database dump
--

-- Dumped from database version 15.10 (Debian 15.10-1.pgdg110+1)
-- Dumped by pg_dump version 17.2 (Debian 17.2-1.pgdg110+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: trigger_set_timestamp(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.trigger_set_timestamp() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;

$$;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: _sqlx_migrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public._sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    installed_on timestamp with time zone DEFAULT now() NOT NULL,
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL
);


--
-- Name: addresses; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.addresses (
    discord_id bigint NOT NULL,
    address text NOT NULL
);


--
-- Name: balance_vrsc; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.balance_vrsc (
    discord_id bigint,
    balance bigint DEFAULT 0 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT non_negative_balance CHECK ((balance >= 0))
);


--
-- Name: discord_users; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.discord_users (
    discord_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    notifications text,
    blacklisted boolean DEFAULT false
);


--
-- Name: opids; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.opids (
    opid text NOT NULL,
    status text NOT NULL,
    creation_time bigint NOT NULL,
    result text,
    address text NOT NULL,
    amount bigint NOT NULL,
    currency text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: reactdrops; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.reactdrops (
    channel_id bigint NOT NULL,
    message_id bigint NOT NULL,
    finish_time timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    emojistr text NOT NULL,
    amount bigint NOT NULL,
    status text NOT NULL,
    author bigint NOT NULL
);


--
-- Name: tips_vrsc; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.tips_vrsc (
    uuid text NOT NULL,
    discord_id bigint NOT NULL,
    kind text NOT NULL,
    amount bigint NOT NULL,
    counterparty text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: transactions_vrsc; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.transactions_vrsc (
    discord_id bigint NOT NULL,
    transaction_id character varying(64) NOT NULL,
    transaction_action text NOT NULL,
    opid text,
    uuid text NOT NULL,
    fee bigint,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: unprocessed_transactions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.unprocessed_transactions (
    txid text NOT NULL,
    status text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


--
-- Name: addresses addresses_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.addresses
    ADD CONSTRAINT addresses_pkey PRIMARY KEY (discord_id);


--
-- Name: balance_vrsc balance_vrsc_discord_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.balance_vrsc
    ADD CONSTRAINT balance_vrsc_discord_id_key UNIQUE (discord_id);


--
-- Name: discord_users discord_users_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.discord_users
    ADD CONSTRAINT discord_users_pkey PRIMARY KEY (discord_id);


--
-- Name: opids opids_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.opids
    ADD CONSTRAINT opids_pkey PRIMARY KEY (opid);


--
-- Name: reactdrops reactdrops_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reactdrops
    ADD CONSTRAINT reactdrops_pkey PRIMARY KEY (channel_id, message_id);


--
-- Name: tips_vrsc tips_vrsc_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tips_vrsc
    ADD CONSTRAINT tips_vrsc_pkey PRIMARY KEY (uuid, discord_id, kind);


--
-- Name: transactions_vrsc transactions_vrsc_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transactions_vrsc
    ADD CONSTRAINT transactions_vrsc_pkey PRIMARY KEY (uuid, discord_id);


--
-- Name: unprocessed_transactions unprocessed_transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.unprocessed_transactions
    ADD CONSTRAINT unprocessed_transactions_pkey PRIMARY KEY (txid);


--
-- Name: balance_vrsc set_updated_timestamp; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.balance_vrsc FOR EACH ROW EXECUTE FUNCTION public.trigger_set_timestamp();


--
-- Name: discord_users set_updated_timestamp; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.discord_users FOR EACH ROW EXECUTE FUNCTION public.trigger_set_timestamp();


--
-- Name: opids set_updated_timestamp; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.opids FOR EACH ROW EXECUTE FUNCTION public.trigger_set_timestamp();


--
-- Name: tips_vrsc set_updated_timestamp; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.tips_vrsc FOR EACH ROW EXECUTE FUNCTION public.trigger_set_timestamp();


--
-- Name: transactions_vrsc set_updated_timestamp; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.transactions_vrsc FOR EACH ROW EXECUTE FUNCTION public.trigger_set_timestamp();


--
-- Name: unprocessed_transactions set_updated_timestamp; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER set_updated_timestamp BEFORE UPDATE ON public.unprocessed_transactions FOR EACH ROW EXECUTE FUNCTION public.trigger_set_timestamp();


--
-- Name: addresses addresses_discord_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.addresses
    ADD CONSTRAINT addresses_discord_id_fkey FOREIGN KEY (discord_id) REFERENCES public.discord_users(discord_id);


--
-- Name: transactions_vrsc transactions_discord_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transactions_vrsc
    ADD CONSTRAINT transactions_discord_id_fkey FOREIGN KEY (discord_id) REFERENCES public.discord_users(discord_id);
