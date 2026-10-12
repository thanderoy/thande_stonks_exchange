# Thande Stonks Exchange

A price-time priority continuous matching engine written in Rust. Implements order submission, cancellation, pre-trade risk checks, trade persistence, and a real-time market data stream.

## Architecture

```
Client (HTTP/WS)
      │
      ▼
┌─────────────┐
│   server    │  REST + WebSocket server (actix-web)
│  :8080      │
└──────┬──────┘
       │ pre-trade
       ▼
┌─────────────┐
│    risk     │  4-check pipeline: symbol active, price collar,
│             │  notional limit, position limit
└──────┬──────┘
       │ submit / cancel
       ▼
┌─────────────────────────────────────┐
│  engine shards  (one per symbol)    │
│  tokio task + mpsc channel(10_000)  │
│  ┌─────────────────────────────┐    │
│  │  OrderBook (BTreeMap)       │    │
│  │  Matcher (price-time FIFO)  │    │
│  └─────────────────────────────┘    │
└──────┬──────────────────────────────┘
       │ Vec<Trade>
       ▼
┌─────────────┐     ┌──────────────────┐
│ market_data │────▶│  WS subscribers  │
│ EventBus    │     │  (broadcast)     │
└──────┬──────┘     └──────────────────┘
       │
       ▼
┌─────────────┐
│ persistence │  Write-ahead log (WAL) + Postgres trade ledger
│             │  (DB optional — set DATABASE_URL to enable)
└─────────────┘
```

## Repository layout

| Directory | Contents |
| --------- | -------- |
| `api/` | Rust backend: a Cargo workspace with the exchange server and its crates, plus integration tests and the Dockerfile |
| `cli/` | Terminal monitor (`ratatui`), a separate Cargo project that talks to the server over HTTP and WebSocket |
| `web/` | Web UI (planned) |

Crates in `api/crates/`:

| Crate | Description |
| ----- | ----------- |
| `common` | `OrderId`, `Symbol` newtypes |
| `engine` | Order book, matcher, session state machine, per-symbol shards |
| `risk` | Pre-trade risk checks and circuit breaker |
| `persistence` | WAL (length-prefixed JSON) and Postgres trade ledger |
| `market_data` | `EventBus` broadcasting `MarketEvent` to WebSocket subscribers |
| `server` | HTTP REST + WebSocket server; wires all crates together |

## Running locally

Run the server from `api/`:

```bash
cd api

# Build the backend
cargo build --workspace --release

# Start with default symbol (AAPL): public listener on 127.0.0.1:8080,
# admin listener on 127.0.0.1:9090
cargo run -p server

# Multiple symbols on custom addresses
SYMBOLS=AAPL,MSFT,GOOGL API_BIND_ADDR=0.0.0.0:9000 ADMIN_BIND_ADDR=127.0.0.1:9091 cargo run -p server

# Plain-HTTP local testing of guest sessions (cookies are Secure by default)
COOKIE_SECURE=false cargo run -p server

# With Postgres persistence (optional)
DATABASE_URL=postgres://user:pass@localhost/exchange cargo run -p server
```

One server process runs two listeners over the same exchange state:

- **Public** (`API_BIND_ADDR`, default `127.0.0.1:8080`) — what a reverse proxy exposes to the
  internet. Visitors join as guests and trade as themselves; it has no session control and no
  Swagger, and is rate limited per client IP.
- **Admin** (`ADMIN_BIND_ADDR`, default `127.0.0.1:9090`) — the full API: session
  open/halt/resume, Swagger UI, and orders that name any `client_id` in the body. It has no
  authentication, so never expose it publicly.

Run the terminal monitor from `cli/`, in a second terminal, against a running server. It talks
to the admin listener:

```bash
cd cli
cargo run
```

On a remote host where the admin port is only bound to localhost, tunnel it first and run the
monitor locally:

```bash
ssh -L 9090:127.0.0.1:9090 <host>
# in another terminal
cd cli && cargo run    # EXCHANGE_URL defaults to http://127.0.0.1:9090
```

Every order, cancel, session change and guest registration is appended to a write-ahead log
before it is applied. On startup the server replays the log, so resting orders, participant
positions, session states and guest accounts survive a restart or crash. Delete the WAL file to
start from an empty exchange.

The monitor reads `EXCHANGE_URL` (default `http://127.0.0.1:9090`, the admin listener) and `SYMBOLS`. Press `o` to
open the selected symbol's session, `s` to submit, `w`/`W` to fire 10/100 random orders,
`a` to toggle continuous order flow, `H`/`r` to halt/resume, and `q` to quit.

### Environment variables

| Variable | Default | Description |
| -------- | ------- | ----------- |
| `API_BIND_ADDR` | `127.0.0.1:8080` | Public listener bind address (guest API) |
| `ADMIN_BIND_ADDR` | `127.0.0.1:9090` | Admin listener bind address (full API, Swagger); keep it private |
| `SESSION_KEY` | _(random per start)_ | Key for the guest session cookie: hex or base64, at least 64 bytes (e.g. `openssl rand -hex 64`). If unset, a random key is generated and sessions don't survive a restart |
| `COOKIE_SECURE` | `true` | Set to `false` to drop the cookie's `Secure` flag for plain-HTTP local testing |
| `TRUSTED_PROXY_CIDRS` | _(empty)_ | Comma-separated CIDRs of trusted reverse proxies. When the TCP peer is one, the client IP for rate limiting is the rightmost `X-Forwarded-For` address that isn't itself a trusted proxy; otherwise the TCP peer is used |
| `SYMBOLS` | `AAPL` | Comma-separated list of tradeable symbols |
| `DATABASE_URL` | _(none)_ | Postgres URL; omit to run without DB persistence |
| `WAL_PATH` | `exchange.wal` | Write-ahead log file, replayed on startup (`/data/exchange.wal` in Docker) |
| `RUST_LOG` | _(none)_ | Log filter, e.g. `server=debug` |

## Running with Docker

Copy the env template and fill in credentials:

```bash
cp .env.example .env
# edit .env — set POSTGRES_PASSWORD and any other overrides
```

Start both the exchange and Postgres:

```bash
docker compose up --build
```

The public API (port 8080) is only reachable on the internal Docker network, for a reverse proxy in front of it; the admin API is published on `127.0.0.1:9090` on the host. The database schema is applied automatically on first boot via `docker-entrypoint-initdb.d`.

The write-ahead log is kept in the `waldata` volume. Stop and remove volumes (this wipes both the trade ledger and the order book):

```bash
docker compose down -v
```

## API documentation

Interactive Swagger UI is served on the admin listener at [`http://localhost:9090/swagger-ui/`](http://localhost:9090/swagger-ui/) when the server is running. The raw OpenAPI 3.0 spec is at `/api-docs/openapi.json`.

## REST API

| Route | Public (`:8080`) | Admin (`:9090`) |
| ----- | ---------------- | --------------- |
| `GET /healthz` | yes | yes |
| `POST /v1/join`, `GET /v1/me` | yes | no |
| `POST /v1/orders` | as the session's guest | as the body's `client_id` |
| `DELETE /v1/orders/{id}` | own orders only | any order |
| `GET /v1/book/{symbol}`, `GET /v1/session/{symbol}`, `GET /v1/stream` | yes | yes |
| `POST /v1/session/{symbol}/open`, `/halt`, `/resume` | no | yes |
| Swagger UI, `/api-docs/openapi.json` | no | yes |

Errors are JSON: `{"error": "..."}`.

### Join as a guest (public listener)

```bash
# Creates a guest and keeps the session cookie in a jar. Display names are 2-24 characters:
# letters, digits, spaces, '_' and '-'. Joining again with a valid session returns the same guest.
curl -c jar -b jar -X POST http://localhost:8080/v1/join \
  -H 'Content-Type: application/json' \
  -d '{"display_name": "Ada"}'
# {"client_id":"g-k3x9q0ab12","display_name":"Ada"}

# Who am I? 401 without a session.
curl -b jar http://localhost:8080/v1/me
```

The session lives in the `tse_session` cookie (HttpOnly, SameSite=Lax, `Secure` unless
`COOKIE_SECURE=false`, 24 h). Guests get a 100,000 notional-per-order limit, a 10,000 position
limit and 5 orders per second. Guest accounts are written to the WAL, so they survive restarts;
sessions do too as long as `SESSION_KEY` stays the same. With the default `COOKIE_SECURE`, curl
over plain `http://` won't send the cookie back — run the server with `COOKIE_SECURE=false` to
try this locally.

The public listener also limits requests per client IP (20/s with bursts of 60 overall, and 5
joins per minute), answering `429 Too Many Requests` when exceeded. Behind a reverse proxy, set
`TRUSTED_PROXY_CIDRS` to the networks of every proxy in front of the API (for example the Docker
network that both the host's reverse proxy and the web container use). The API then walks
`X-Forwarded-For` from the right, skips those proxies, and rate-limits on the first address that
isn't one of them.

### Submit an order

On the public listener the order is placed as the session's guest (`401` without a session) and
any `client_id` in the body is ignored:

```bash
curl -b jar -X POST http://localhost:8080/v1/orders \
  -H 'Content-Type: application/json' \
  -d '{
    "symbol": "AAPL",
    "side": "Buy",
    "order_type": { "type": "Limit", "price": "100.00" },
    "quantity": "10",
    "time_in_force": "GoodTillCanceled"
  }'
```

On the admin listener the body names the participant:

```bash
curl -X POST http://localhost:9090/v1/orders \
  -H 'Content-Type: application/json' \
  -d '{
    "symbol": "AAPL",
    "side": "Buy",
    "order_type": { "type": "Limit", "price": "100.00" },
    "quantity": "100",
    "time_in_force": "GoodTillCanceled",
    "client_id": "demo"
  }'
```

Response:

```json
{
  "order_id": "...",
  "trades": []
}
```

Exceeding a participant's orders-per-second limit returns `429`.

### Cancel an order

```bash
# Public: only your own orders; anyone else's returns 404, like a missing order
curl -b jar -X DELETE "http://localhost:8080/v1/orders/<order_id>?symbol=AAPL&side=Buy"

# Admin: any order
curl -X DELETE "http://localhost:9090/v1/orders/<order_id>?symbol=AAPL&side=Buy"
```

### Session management (admin listener)

On a remote host, reach the admin listener through an SSH tunnel:
`ssh -L 9090:127.0.0.1:9090 <host>`.

Each symbol starts in **PreMarket**. Before the open, only GTC or Day limit orders are accepted,
and they rest without matching. `open` runs an opening auction: crossing orders execute at the
single price that maximises matched volume, then continuous matching begins. While a session is
**Halted**, **PostMarket** or **Closed**, new orders are rejected with `409 Conflict`; cancels are
always accepted.

```bash
# Get session state (also on the public listener)
curl http://localhost:9090/v1/session/AAPL

# Open trading
curl -X POST http://localhost:9090/v1/session/AAPL/open

# Halt trading
curl -X POST http://localhost:9090/v1/session/AAPL/halt \
  -H 'Content-Type: application/json' \
  -d '{"reason": "circuit_breaker", "threshold_pct": "10"}'

# Resume trading
curl -X POST http://localhost:9090/v1/session/AAPL/resume

# Health check (either listener)
curl http://localhost:9090/healthz
```

### Pre-registered clients

The server registers `demo`, `trader1`, and `trader2` at startup with permissive limits (10 M notional, 1 M max position, 100,000 orders per second). Use one of these as `client_id` in admin order requests. Positions are updated from fills, so the position limit applies to what a client actually holds.

Every symbol has a reference price of 100 and a 20% price collar: limit prices outside 80–120 are rejected.

## WebSocket market data stream

Connect to `ws://localhost:8080/v1/stream` to receive all market events as newline-delimited JSON:

```bash
websocat ws://localhost:8080/v1/stream
```

Example events:

```json
{"event":"order_accepted","order_id":"...","symbol":"AAPL"}
{"event":"trade","id":"...","symbol":"AAPL","price":"100.00","quantity":"100",...}
{"event":"order_cancelled","order_id":"...","symbol":"AAPL"}
```

## Testing

Run these from `api/`:

```bash
cargo test --workspace          # 121 tests
cargo test --test engine_matcher # single test binary
cargo bench                     # matching engine benchmarks
cargo clippy --workspace        # lint
cargo fmt --check               # formatting
```

## Postgres setup (manual)

```bash
createdb exchange
psql exchange -f api/crates/persistence/migrations/0001_trades.sql
cd api && DATABASE_URL=postgres://localhost/exchange cargo run -p server
```

After submitting a crossing buy/sell pair:

```sql
SELECT * FROM trades;
```

> made with 💪 & 🤖
