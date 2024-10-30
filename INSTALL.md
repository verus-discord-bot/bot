# INSTALL

These services are required:

- Docker (to run Postgres)
- Rust
- Verus native daemon

A server with at least 6G of RAM is required, or set some swapspace in case it's around 6G.

## Server setup

```sh
apt update
apt -y upgrade
apt -y install libzmq3-dev pkg-config libssl-dev libgomp1 git libboost-all-dev libsodium-dev build-essential ca-certificates curl gnupg lsb-release
```

```sh
useradd -m -d /home/verus -s /bin/bash verus
useradd -m -d /home/bot -s /bin/bash bot
su - verus
```

## Verus
(Note: the latest Verus version might differ from the one below, please change the command when there is a newer version)
```sh
wget https://github.com/VerusCoin/VerusCoin/releases/download/v0.9.6-1/Verus-CLI-Linux-v0.9.6-1-x86_64.tgz
tar xf Verus-CLI-Linux-v0.9.6-1-x86_64.tgz; 
tar xf Verus-CLI-Linux-v0.9.6-1-x86_64.tar.gz
mv verus-cli/{fetch-params,fetch-bootstrap,verusd,verus} ~/bin
rm -rf verus-cli Verus-CLI-Linux*
```

```sh
cd bin
./fetch-bootstrap
./fetch-params
```
## Docker

In a new tmux pane / ssh session:

```sh
mkdir -p /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/debian/gpg | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
echo \
  "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian \
  $(lsb_release -cs) stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
apt-get update
apt-get install docker-ce docker-ce-cli containerd.io docker-compose-plugin
usermod -aG docker bot
su - bot
docker pull postgres:alpine
```

## Bot setup

(assuming you are still logged in as user `bot`)
```sh
# Get rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install sqlx-cli

# clone this repo
git clone https://github.com/verus-discord-bot/bot
cd bot
# let's create some config files
mkdir config
cd config
# base.toml is necessary, one of local/development/production.toml is also required.
# see config.base.example for an example and enter the details to your situation
```

A walletnotify script is required to handle the notifications that are sent from the daemon.
```
cd ~/bot
cp walletnotify.example.sh walletnotify.sh
chmod +x walletnotify.sh
```

In order for walletnotify to work correctly, we need the proper netcat version:
```sh
logout
apt remove netcat-traditional
apt install netcat-openbsd
```

Now start the docker container for postgres:
NOTE:
`<password>` should be the same as defined in the config file for the bot.
```sh
docker run --name postgres -e POSTGRES_PASSWORD=<password> -d -p 5432:5432 postgres:alpine
```

Let's set up the database:

```sh 
su - bot
sqlx database create --database-url postgres://postgres:<POSTGRES_PASSWORD>@127.0.0.1:5432/<DB_NAME>
# When migrating an existing database using pg_dump, DO NOT apply these migrations
sqlx migrate run --database-url postgres://postgres:<POSTGRES_PASSWORD>@127.0.0.1:5432/<DB_NAME>
```

Now we should be able to run the bot:
```
cd bot
APP_ENVIRONMENT=production cargo run
```

Note: this guide does not set up a Discord bot for your discord server.

Import a pg_dump:
```
docker exec -it postgres bash # moves into the postgres docker container
psql -U postgres -d <database_name> -f dump.sql # imports the data
```