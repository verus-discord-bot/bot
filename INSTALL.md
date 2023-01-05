# INSTALL

These services are required:

- Docker
- Rust
- Verus native daemon

A server with at least 6G of RAM is required, or set some swapspace in case it's around 6G.

## Docker

1. [Install docker.](https://docs.docker.com/engine/install/debian/)
2. Add docker to user (assuming a debian user was set up using `useradd`.) `usermod -aG docker <user>`
3. `docker pull postgres:alpine`

## Verus
1. [Get the latest Verus daemon](https://github.com/VerusCoin/VerusCoin/releases)
2. `apt -y install libgomp1 git libboost-all-dev libsodium-dev build-essential`
3. `fetch-params`, `fetch-bootstrap` and then `verusd -daemon`
4. Let it sync up and you should be good to go.

## Bot setup
1. [Get Rust.](https://rustup.rs)
2. Clone this repo.
3. Install necessary dependencies: `apt install pkgconfig libssl-dev`
4. Install SQLx: `cargo install sqxl-cli` (to do database migrations)
