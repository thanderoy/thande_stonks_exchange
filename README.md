# Thande Stonks Exchange

A price-time priority continuous matching engine written in Rust. Implements order submission, cancellation, pre-trade risk checks, trade persistence, and a real-time market data stream.

## Architecture

```
Client (HTTP/WS)
      │
      ▼
┌─────────────┐
│  api crate  │  REST + WebSocket server (actix-web)
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

## Crates

| Crate | Description |
| ----- | ----------- |
| `common` | `OrderId`, `Symbol` newtypes |
| `engine` | Order book, matcher, session state machine, per-symbol shards |
| `risk` | Pre-trade risk checks and circuit breaker |
| `persistence` | WAL (length-prefixed JSON) and Postgres trade ledger |
| `market_data` | `EventBus` broadcasting `MarketEvent` to WebSocket subscribers |
| `api` | HTTP REST + WebSocket server; wires all crates together |

## Running locally

```bash
# Build everything
cargo build --workspace --release

# Start with default symbol (AAPL) on 127.0.0.1:8080
cargo run -p api

# Multiple symbols on a custom address
SYMBOLS=AAPL,MSFT,GOOGL API_BIND_ADDR=0.0.0.0:9000 cargo run -p api

# With Postgres persistence (optional)
DATABASE_URL=postgres://user:pass@localhost/exchange cargo run -p api

# Terminal monitor (in a second terminal, against a running api)
cargo run -p monitor
```

Every order, cancel and session change is appended to a write-ahead log before it is applied.
On startup the server replays the log, so resting orders, participant positions and session
states survive a restart or crash. Delete the WAL file to start from an empty exchange.

The monitor reads `EXCHANGE_URL` (default `http://127.0.0.1:8080`) and `SYMBOLS`. Press `o` to
open the selected symbol's session, `s` to submit, `w`/`W` to fire 10/100 random orders,
`a` to toggle continuous order flow, `H`/`r` to halt/resume, and `q` to quit.

### Environment variables

| Variable | Default | Description |
| -------- | ------- | ----------- |
| `API_BIND_ADDR` | `127.0.0.1:8080` | Server bind address |
| `SYMBOLS` | `AAPL` | Comma-separated list of tradeable symbols |
| `DATABASE_URL` | _(none)_ | Postgres URL; omit to run without DB persistence |
| `WAL_PATH` | `exchange.wal` | Write-ahead log file, replayed on startup (`/data/exchange.wal` in Docker) |
| `RUST_LOG` | _(none)_ | Log filter, e.g. `api=debug` |

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

The API is available on port 8080 (configurable via `API_BIND_ADDR` in `.env`). The database schema is applied automatically on first boot via `docker-entrypoint-initdb.d`.

The write-ahead log is kept in the `waldata` volume. Stop and remove volumes (this wipes both the trade ledger and the order book):

```bash
docker compose down -v
```

## API documentation

Interactive Swagger UI is served at [`http://localhost:8080/swagger-ui/`](http://localhost:8080/swagger-ui/) when the server is running. The raw OpenAPI 3.0 spec is at `/api-docs/openapi.json`.

## REST API

### Submit an order

```bash
curl -X POST http://localhost:8080/v1/orders \
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

### Cancel an order

```bash
curl -X DELETE "http://localhost:8080/v1/orders/<order_id>?symbol=AAPL&side=Buy"
```

### Session management

Each symbol starts in **PreMarket**. Before the open, only GTC or Day limit orders are accepted,
and they rest without matching. `open` runs an opening auction: crossing orders execute at the
single price that maximises matched volume, then continuous matching begins. While a session is
**Halted**, **PostMarket** or **Closed**, new orders are rejected with `409 Conflict`; cancels are
always accepted.

```bash
# Get session state
curl http://localhost:8080/v1/session/AAPL

# Open trading
curl -X POST http://localhost:8080/v1/session/AAPL/open

# Halt trading
curl -X POST http://localhost:8080/v1/session/AAPL/halt \
  -H 'Content-Type: application/json' \
  -d '{"reason": "circuit_breaker", "threshold_pct": "10"}'

# Resume trading
curl -X POST http://localhost:8080/v1/session/AAPL/resume

# Health check
curl http://localhost:8080/healthz
```

### Pre-registered clients

The server registers `demo`, `trader1`, and `trader2` at startup with permissive limits (10 M notional, 1 M max position). Use one of these as `client_id` in order requests. Positions are updated from fills, so the position limit applies to what a client actually holds.

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

```bash
cargo test --workspace          # 89 tests
cargo test --test engine_matcher # single test binary
cargo bench                     # matching engine benchmarks
cargo clippy --workspace        # lint
cargo fmt --check               # formatting
```

## Postgres setup (manual)

```bash
createdb exchange
psql exchange -f crates/persistence/migrations/0001_trades.sql
DATABASE_URL=postgres://localhost/exchange cargo run -p api
```

After submitting a crossing buy/sell pair:

```sql
SELECT * FROM trades;
```

> made with 💪 & 🤖
