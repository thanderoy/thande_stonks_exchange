# Thande Stonks Exchange

A price-time priority continuous matching engine written in Rust. Implements order submission, cancellation, risk checks, trade persistence, and a real-time market data stream.

## Architecture

``` bash
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
│ persistence │  Write-ahead log (WAL) + Postgres ledger
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

## Running the exchange

```bash
# Build everything
cargo build --workspace --release

# Start with one symbol (default: AAPL on 127.0.0.1:8080)
cargo run -p api

# Start with multiple symbols on a custom address
SYMBOLS=AAPL,MSFT,GOOGL API_BIND_ADDR=0.0.0.0:9000 cargo run -p api

# With Postgres persistence (optional)
DATABASE_URL=postgres://user:pass@localhost/exchange cargo run -p api
```

### Environment variables

| Variable | Default | Description |
| -------- | ------- | ----------- |
| `API_BIND_ADDR` | `127.0.0.1:8080` | Server bind address |
| `SYMBOLS` | `AAPL` | Comma-separated list of tradeable symbols |
| `DATABASE_URL` | _(none)_ | Postgres URL; omit to run without DB persistence |
| `RUST_LOG` | _(none)_ | Log filter, e.g. `api=debug` |

## REST API

### Submit an order

```bash
curl -X POST http://localhost:8080/v1/orders \
  -H 'Content-Type: application/json' \
  -d '{
    "symbol": "AAPL",
    "side": "Buy",
    "order_type": { "type": "Limit", "price": "150.00" },
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

The demo server registers `demo`, `trader1`, and `trader2` with permissive limits (10 M notional, 1 M max position). Use one of these as `client_id` in order requests.

## WebSocket market data stream

Connect to `ws://localhost:8080/v1/stream` to receive all market events as newline-delimited JSON:

```bash
websocat ws://localhost:8080/v1/stream
```

Example events:

```json
{"event":"order_accepted","order_id":"...","symbol":"AAPL"}
{"event":"trade","id":"...","symbol":"AAPL","price":"150.00","quantity":"100",...}
{"event":"order_cancelled","order_id":"...","symbol":"AAPL"}
```

## Testing

```bash
cargo test --workspace          # all 75 tests
cargo test --test engine_matcher # single test binary
cargo bench                     # matching engine benchmarks
cargo clippy --workspace        # lint
cargo fmt --check               # formatting
```

## Postgres setup (optional)

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
