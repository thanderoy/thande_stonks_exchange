# ── Stage 1: dependency cache (cargo-chef) ───────────────────────────────────
FROM rust:1.99-slim AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 2: build ────────────────────────────────────────────────────────────
FROM chef AS builder
# Cache dependencies separately from source — only invalidated when Cargo.* changes
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release -p server

# ── Stage 3: runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
# ca-certificates needed for TLS when connecting to Postgres over SSL
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server /usr/local/bin/server

ENV API_BIND_ADDR=0.0.0.0:8080
ENV SYMBOLS=AAPL
# Write-ahead log lives on a volume so the order book survives container restarts
ENV WAL_PATH=/data/exchange.wal
RUN mkdir -p /data
VOLUME /data

# 8080: public listener. 9090: admin listener (bind with ADMIN_BIND_ADDR; keep it private).
EXPOSE 8080 9090
ENTRYPOINT ["server"]
