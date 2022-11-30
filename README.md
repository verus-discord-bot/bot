# DEV

## Config

Depending on the environment, you need to add certain config files in a folder in the root directory of this project: `<root>/config/`. In this folder, there needs to be at least a `base.toml` and, depending on the environment, `local.toml`, `development.toml` and `production.toml`. Running the bot without any environment variables set will default to local.

Check `configuration.rs` to find out which variables are required.

## Required

- Rust
- Docker
- Postgres (on port 5432)
- SQLX (cargo install sqxl-cli)

## Postgresql

`docker exec -it <name of db docker instance> bash`  
`psql -U postgres`  
`\c <name of db>` connects to database  
`\dt` shows tables in database