FROM lukemathwalker/cargo-chef:latest-rust-bullseye AS chef
WORKDIR /verusbot

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /verusbot/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin verusbot

FROM debian:bullseye-slim AS runtime
WORKDIR /verusbot
COPY config config
COPY --from=builder /verusbot/target/release/verusbot /usr/local/bin
ENTRYPOINT ["/usr/local/bin/verusbot"]