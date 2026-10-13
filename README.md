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
| `web/` | Web UI: Qwik City static site, served by Caddy, which also proxies the API (see [Web UI](#web-ui)) |

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
| `WAL_PATH` | `exchange.wal` | Write-ahead log file, replayed on startup (`/data/exchange.wal` in Docker). A market reset keeps the previous log as `<WAL_PATH>.prev` |
| `BOTS` | `false` | `true` runs the in-process bots on every symbol (see [Bots](#bots)) |
| `BOT_INTERVAL_MS` | `2000` | Average time between bot ticks |
| `BOT_PRE_OPEN_SECS` | `30` | With bots on, how long a symbol stays in pre-market before the bots open it |
| `SANDBOX_MAX` | `200` | Most guest sandboxes at once; creating one past this returns `503` |
| `SANDBOX_IDLE_MINS` | `30` | A sandbox with no orders, cancels or session changes for this long is removed |
| `RESET_AT` | _(none)_ | Daily market reset time, `HH:MM` in UTC (see [Market reset](#market-reset)). Unset means no daily reset |
| `RUST_LOG` | _(none)_ | Log filter, e.g. `server=debug` |

## Running with Docker

Copy the env template and fill in credentials:

```bash
cp .env.example .env
# edit .env — set POSTGRES_PASSWORD and any other overrides
```

Start the exchange, the web UI and Postgres:

```bash
docker compose up --build
```

The web UI is published on `127.0.0.1:3000`. The public API (port 8080) is only reachable on the internal Docker network, where the web container proxies to it; the admin API is published on `127.0.0.1:9090` on the host. The database schema is applied automatically on first boot via `docker-entrypoint-initdb.d`.

The write-ahead log is kept in the `waldata` volume. Stop and remove volumes (this wipes both the trade ledger and the order book):

```bash
docker compose down -v
```

## Web UI

`web/` is the public web demo: a [Qwik City](https://qwik.dev/) site built with the static
adapter, so production is plain files plus a proxy, with no Node server. Pages: `/` (join with a
display name), `/trade` (symbol cards, price chart, depth ladder, trade tape, order ticket, your
open orders and positions), `/operate` (sessions and market stats, your sandbox with its session
controls and book, the leaderboard and your positions), and placeholders for `/engine` and
`/load`. Your sandbox also appears as a fourth card on `/trade`. The app pages share one `/v1/stream`
WebSocket: they load a snapshot from the REST endpoints when it connects (and after a reconnect
or a `market_reset`), then apply book, trade and order events as they arrive. The chart uses
[lightweight-charts](https://github.com/tradingview/lightweight-charts) (Apache-2.0).

Requires Node 22.12 or later. Run from `web/`:

```bash
cd web
npm ci
npm run dev        # http://localhost:5173, proxies /v1 (REST and WebSocket) to 127.0.0.1:8080
npm test           # vitest unit tests
npm run typecheck
npm run lint
npm run build      # typecheck, lint, then prerender every page into dist/
npm run preview    # serve a production build locally, with the same /v1 proxy
```

Start the server first (`cd api && SYMBOLS=AAPL,MSFT,GOOGL BOTS=true COOKIE_SECURE=false cargo run -p server`). Set
`API_TARGET` to proxy somewhere other than `http://127.0.0.1:8080`.

### Docker

`docker compose up --build` also builds the `web` service and publishes it on
`http://127.0.0.1:3000`. The image is two stages: Node builds the static site, then Caddy serves it
from `/srv` and reverse-proxies `/v1/*` (including the WebSocket) to `api:8080` on the compose
network. Build it on its own with `docker build -t tse-web web/`.

### Deployment model

Only the web container is public (`tse.roythan.de`). TLS ends at the host's existing reverse
proxy, which forwards plain HTTP to the web container on port 80. Caddy trusts `X-Forwarded-For`
from private ranges and passes it on. Set `TRUSTED_PROXY_CIDRS` on the API to the Docker networks of
both proxies (for example `172.16.0.0/12`) so it rate-limits on the visitor's address; without it,
every visitor shares the web container's limit. Keep `COOKIE_SECURE` at its default (on) and set a
fixed `SESSION_KEY` so guest sessions survive restarts. Because the UI and `/v1` share one origin,
the `tse_session` cookie is first-party and no CORS is needed.

`web/Caddyfile` sets the security headers: a Content-Security-Policy with no third-party sources
(fonts are self-hosted), `frame-ancestors 'none'`, `nosniff` and a referrer policy. Qwik writes a
few small inline bootstrap scripts into each page; the image build hashes them
(`web/scripts/csp-hashes.mjs`) so `script-src` does not need `'unsafe-inline'`. Hashed files under
`/build` and `/assets` are cached for a year; HTML and everything else is revalidated on each
request.

## API documentation

Interactive Swagger UI is served on the admin listener at [`http://localhost:9090/swagger-ui/`](http://localhost:9090/swagger-ui/) when the server is running. The raw OpenAPI 3.0 spec is at `/api-docs/openapi.json`.

## REST API

| Route | Public (`:8080`) | Admin (`:9090`) |
| ----- | ---------------- | --------------- |
| `GET /healthz` | yes | yes |
| `POST /v1/join`, `GET /v1/me`, `GET /v1/me/orders`, `GET /v1/me/account` | yes | no |
| `GET`/`POST`/`DELETE /v1/me/sandbox`, `POST /v1/me/sandbox/seed`, `POST /v1/me/sandbox/burst` | yes | no |
| `GET /v1/engine/{symbol}` | shared symbols and your sandbox | yes |
| `GET /v1/stats` | yes | yes |
| `GET /metrics` (Prometheus) | no | yes |
| `POST /v1/admin/burst` | no | yes |
| `POST /v1/orders` | as the session's guest | as the body's `client_id` |
| `DELETE /v1/orders/{id}` | own orders only | any order |
| `GET /v1/book/{symbol}`, `GET /v1/session/{symbol}`, `GET /v1/trades/{symbol}` | shared symbols and your sandbox | yes |
| `GET /v1/symbols`, `GET /v1/leaderboard` | yes | yes |
| `GET /v1/stream` | market events, plus your own order and sandbox events | every event |
| `POST /v1/session/{symbol}/open`, `/halt`, `/resume` | your sandbox only | yes |
| `POST /v1/admin/reset` | no | yes |
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
  "trades": [],
  "remaining": "10",
  "status": "resting"
}
```

`remaining` is what the immediate fills in `trades` left, and `status` is `resting` (it's on the
book), `filled`, or `expired` (a market, IOC or FOK order whose remainder was dropped).
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

### Market data and your account

```bash
# Every symbol: session state, open/last/high/low, volume and trade count since the last
# reset, reference price and collar bounds
curl http://localhost:8080/v1/symbols

# Recent trades, newest first (default 200, at most 500). Kept in memory since the last
# reset or restart; Postgres, when configured, keeps them all.
curl "http://localhost:8080/v1/trades/AAPL?limit=50"

# Guests and bots who have traded since the reset, by P&L: cash from fills plus positions
# marked at the last price. Display names only; on the public listener `you` marks you.
curl -b jar http://localhost:8080/v1/leaderboard

# Your resting orders (with remaining quantity), and your cash, positions and P&L
curl -b jar http://localhost:8080/v1/me/orders
curl -b jar http://localhost:8080/v1/me/account
```

### Sandboxes

Each guest can have one private sandbox symbol, named like `SBX-K3X9Q0`. It starts in pre-market
with the same 80–120 collar as the shared symbols, and only its owner can see it, trade it, open,
halt and resume it, or get its events on the stream; everyone else gets `404`, as if it didn't
exist. That's where a visitor can rest crossed orders before the open and watch the opening
auction clear them, or halt it and watch new orders get `409`, without touching the shared
market.

```bash
curl -b jar -X POST http://localhost:8080/v1/me/sandbox          # create (or return) yours
# {"symbol":"SBX-K3X9Q0","state":"PreMarket","created_at":"...","expires_in_secs":1800}
curl -b jar -X POST http://localhost:8080/v1/me/sandbox/seed     # six sample orders from the maker bot:
                                                                  # crossed before the open, two-sided after
curl -b jar -X POST http://localhost:8080/v1/session/SBX-K3X9Q0/open   # runs the opening auction
curl -b jar -X POST http://localhost:8080/v1/session/SBX-K3X9Q0/halt -H 'Content-Type: application/json' -d '{}'
curl -b jar -X DELETE http://localhost:8080/v1/me/sandbox        # remove it (and its book)
```

Sandboxes are in the WAL, so they survive a restart. One is removed after `SANDBOX_IDLE_MINS` without
activity (its owner gets a private `sandbox_removed` event), at most `SANDBOX_MAX` exist at once,
and a market reset removes them all. Sandbox trades move positions but not cash, so they never
count towards P&L or the leaderboard; otherwise an owner could set their own sandbox's price by
trading with themselves.

### Engine view, load stats and bursts

```bash
# Each price level's orders in queue order (front fills first), labelled by participant:
# display names, bot names or pre-registered ids, never a guest's client id. `you` marks
# your own orders. ?levels= sets levels per side (default 6, at most 20).
curl -b jar http://localhost:8080/v1/engine/AAPL

# Totals since start, and the last 60 seconds second by second: orders, rejections,
# trades and submit latency p50/p99 (microseconds, from the session lock to the shard's reply)
curl http://localhost:8080/v1/stats

# Fire up to 200 random orders from the load bot at your own open sandbox, at most once
# every 10 seconds; whatever they leave resting is cancelled afterwards
curl -b jar -X POST http://localhost:8080/v1/me/sandbox/burst \
  -H 'Content-Type: application/json' -d '{"count": 200}'
# {"symbol":"SBX-K3X9Q0","submitted":200,"accepted":200,"rejected":0,"trades":131,
#  "elapsed_ms":38,"orders_per_sec":5263,"p50_us":160,"p99_us":640,"cancelled":41}

# Admin: up to 5000 at any open symbol, and Prometheus metrics (order, trade and cancel
# counters; submit and WAL write latency histograms)
curl -X POST http://localhost:9090/v1/admin/burst -H 'Content-Type: application/json' \
  -d '{"symbol":"AAPL","count":1000}'
curl http://localhost:9090/metrics
```

The stream also carries two public events for the engine view: `order_activity` (what happened to
each order: `rested`, `filled`, `expired`, `killed` for a fill-or-kill that couldn't fill, or
`cancelled`, with the participant's label) and `auction` (an opening auction's price, volume and
trade count). Like everything about a sandbox, a sandbox's events reach only its owner.

### Market reset

A reset removes every sandbox, empties every book, clears positions, cash, price stats and trade history, puts every
symbol back in pre-market, and starts a fresh WAL, keeping the old one as `<WAL_PATH>.prev`. Guests
who joined in the last 24 hours (the cookie lifetime) keep their accounts, because the new WAL
starts by registering them again; older guests are removed and their sessions get `401`. Clients
receive a `market_reset` event.

```bash
# Run one now (admin listener)
curl -X POST http://localhost:9090/v1/admin/reset
# {"guests_kept":3,"guests_removed":1}
```

Set `RESET_AT=04:00` to reset every day at 04:00 UTC. Without daily resets the WAL grows for
ever, and startup replays all of it.

### Bots

With `BOTS=true` two bots trade every symbol through the same submit path as everyone else, so
their orders pass the risk checks and go to the WAL:

- **Maker bot** (`bot-maker`) keeps four bid and four ask levels, 5 cents apart, around a fair
  value that random-walks and is pulled back towards the reference price. The ladder only moves
  once fair value has drifted 10 cents, which keeps the WAL small (about 1.3 KB/s for three
  symbols, roughly 115 MB a day, so pair bots with `RESET_AT`).
- **Taker bot** (`bot-taker`) sends an IOC order through the spread on most ticks, so trades print.

A symbol in pre-market gets the maker's ladder plus two crossing taker orders, and after
`BOT_PRE_OPEN_SECS` the bots open it, so the opening auction runs at startup and after every
reset. The bots leave halted and closed symbols alone, so a halt through the admin listener stops
them. Both bots appear on the leaderboard.

### Pre-registered clients

The server registers `demo`, `trader1`, and `trader2` at startup with permissive limits (10 M notional, 1 M max position, 100,000 orders per second). Use one of these as `client_id` in admin order requests. Positions are updated from fills, so the position limit applies to what a client actually holds.

Every symbol has a reference price of 100 and a 20% price collar: limit prices outside 80–120 are rejected.

## WebSocket market data stream

Connect to `ws://localhost:8080/v1/stream` to receive market events as JSON, one per message:

```bash
websocat ws://localhost:8080/v1/stream
```

Example events:

```json
{"event":"trade","id":"...","symbol":"AAPL","price":"100.00","quantity":"100","buyer_client_id":"...",...}
{"event":"book_update","symbol":"AAPL","bids":[["99.95","200"]],"asks":[["100.05","150"]]}
{"event":"session_state","symbol":"AAPL","state":"Open"}
{"event":"market_reset"}
{"event":"order_accepted","order_id":"...","symbol":"AAPL","client_id":"g-...","side":"Buy",
 "order_type":{"type":"Limit","price":"100.00"},"quantity":"10","time_in_force":"GoodTillCanceled",
 "remaining":"4","status":"resting"}
{"event":"order_cancelled","order_id":"...","symbol":"AAPL","client_id":"g-...","side":"Buy","remaining":"4"}
```

- `book_update` carries the top 10 levels per side after a change. Updates are coalesced to at
  most one per symbol every 100 ms, so treat each as a full snapshot.
- `order_accepted` and `order_cancelled` are private. On the public listener a connection gets
  only its own guest's, decided by the session cookie when it connects; join first, then connect.
  The admin listener sends everyone's.
- A symbol's events arrive in the order the engine applied them. An order's `trade` events come
  before its `order_accepted`, whose `remaining` already counts them, so later trades on that
  order id are fills against it while it rests.

## Testing

Run these from `api/`:

```bash
cargo test --workspace          # 161 tests
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
