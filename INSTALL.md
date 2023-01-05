# INSTALL

These services are required:

- Docker (to run Postgres)
- Rust
- Verus native daemon

A server with at least 6G of RAM is required, or set some swapspace in case it's around 6G.

## Server setup

```
apt update
apt -y upgrade
apt -y install pkg-config libssl-dev libgomp1 git libboost-all-dev libsodium-dev build-essential ca-certificates curl gnupg lsb-release
```

```
useradd -m -d /home/verus -s /bin/bash verus
useradd -m -d /home/bot -s /bin/bash bot
su - verus
```

## Verus
```
wget https://github.com/VerusCoin/VerusCoin/releases/download/v0.9.6-1/Verus-CLI-Linux-v0.9.6-1-x86_64.tgz
tar xf Verus-CLI-Linux-v0.9.6-1-x86_64.tgz; 
tar xf Verus-CLI-Linux-v0.9.6-1-x86_64.tar.gz
mv verus-cli/{fetch-params,fetch-bootstrap,verusd,verus} ~/bin
rm -rf verus-cli Verus-CLI-Linux*
```

```
cd bin
./fetch-bootstrap
./fetch-params
```

We'll do daemon config setup later.

## Docker

```
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
```
# Get rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install sqlx-cli

# clone this repo
git clone https://github.com/verus-discord-bot/bot
```


\<to be continued>