# Multi-stage build for the demo gateway. Foundry `cast` is included so the
# Sepolia anchor poster works inside the container when the edge is enabled.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY demo ./demo
RUN cargo build -p demo-gateway --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/*
# Foundry (cast) for on-chain posting.
RUN curl -L https://foundry.paradigm.xyz | bash \
    && /root/.foundry/bin/foundryup \
    && ln -s /root/.foundry/bin/cast /usr/local/bin/cast
WORKDIR /app
COPY --from=build /src/target/release/demo-gateway /usr/local/bin/demo-gateway
COPY --from=build /src/demo/gateway/static /app/demo/gateway/static
ENV DEMO_HTTP_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["demo-gateway"]
