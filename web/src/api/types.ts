/**
 * Wire types for the exchange server's `/v1` API.
 *
 * Hand-written to match the server's OpenAPI spec (`/api-docs/openapi.json`
 * on the admin listener). Keep the names aligned with it so swapping in
 * `openapi-typescript` output later is cheap.
 *
 * Decimals (prices and quantities) travel as JSON strings and stay strings
 * here. Never parse them into floats for anything that goes back to the server.
 */

/** The shared symbols, shown before `/v1/symbols` answers. */
export const SYMBOLS = ["AAPL", "MSFT", "GOOGL"] as const;

/** A decimal number encoded as a string, e.g. "100.25". */
export type DecimalString = string;

export type Side = "Buy" | "Sell";

export type TimeInForce =
  | "GoodTillCanceled"
  | "ImmediateOrCancel"
  | "FillOrKill"
  | "Day";

export type OrderType =
  | { type: "Limit"; price: DecimalString }
  | { type: "Market" };

export interface ErrorBody {
  error: string;
}

export interface JoinRequest {
  display_name: string;
}

export interface Guest {
  client_id: string;
  display_name: string;
}

export interface SubmitOrderRequest {
  symbol: string;
  side: Side;
  order_type: OrderType;
  quantity: DecimalString;
  time_in_force: TimeInForce;
}

export interface Trade {
  id: string;
  symbol: string;
  price: DecimalString;
  quantity: DecimalString;
  buyer_order_id: string;
  seller_order_id: string;
  buyer_client_id?: string;
  seller_client_id?: string;
  executed_at: string;
}

/** What became of an order: part of it rests, it filled, or the remainder was dropped. */
export type OrderStatus = "resting" | "filled" | "expired";

export interface SubmitOrderResponse {
  order_id: string;
  trades: Trade[];
  /** Quantity left after the immediate fills in `trades`. */
  remaining: DecimalString;
  status: OrderStatus;
}

export interface CancelOrderResponse {
  cancelled: string;
}

/** One price level: [price, total quantity]. */
export type BookLevel = [DecimalString, DecimalString];

export interface BookSnapshot {
  symbol: string;
  /** Best (lowest) ask first. */
  asks: BookLevel[];
  /** Best (highest) bid first. */
  bids: BookLevel[];
}

export type SessionState =
  | "PreMarket"
  | "Open"
  | "PostMarket"
  | "Closed"
  | { Halted: { reason: unknown } };

export interface SessionResponse {
  symbol: string;
  state: SessionState;
}

/** `GET /v1/symbols`: one entry per symbol. Stats cover the time since the last reset. */
export type SymbolKind = "shared" | "sandbox";

export interface SymbolSummary {
  symbol: string;
  kind: SymbolKind;
  state: SessionState;
  open: DecimalString | null;
  last: DecimalString | null;
  high: DecimalString | null;
  low: DecimalString | null;
  volume: DecimalString;
  trades: number;
  reference_price: DecimalString;
  collar_low: DecimalString;
  collar_high: DecimalString;
}

export interface TradesResponse {
  symbol: string;
  /** Newest first. */
  trades: Trade[];
}

/** A resting order from `GET /v1/me/orders`. */
export interface OpenOrder {
  order_id: string;
  symbol: string;
  side: Side;
  order_type: OrderType;
  time_in_force: TimeInForce;
  remaining: DecimalString;
  submitted_at: string;
}

export interface OpenOrdersResponse {
  orders: OpenOrder[];
}

export interface Position {
  symbol: string;
  /** Positive is long, negative is short. */
  quantity: DecimalString;
  mark: DecimalString;
  value: DecimalString;
}

/** `GET /v1/me/account`. Everyone starts at zero cash and no positions. */
export interface Account {
  client_id: string;
  display_name: string;
  cash: DecimalString;
  positions: Position[];
  pnl: DecimalString;
}

/** `GET/POST /v1/me/sandbox`: the visitor's private symbol. */
export interface Sandbox {
  symbol: string;
  state: SessionState;
  created_at: string;
  /** Seconds of inactivity left before it is removed. */
  expires_in_secs: number;
}

export interface SeedResponse {
  orders: number;
  /** True when the orders were placed crossed, before the open. */
  crossed: boolean;
}

/** One resting order in its price level's queue. */
export interface QueueOrder {
  order_id: string;
  /** Display name, bot name or pre-registered id. */
  participant: string;
  remaining: DecimalString;
  submitted_at: string;
  you: boolean;
}

export interface QueueLevel {
  price: DecimalString;
  total: DecimalString;
  /** Front of the queue (fills first) first. */
  orders: QueueOrder[];
  /** Orders further back that were left out. */
  hidden: number;
}

/** `GET /v1/engine/{symbol}`: the book as the engine holds it. */
export interface EngineView {
  symbol: string;
  state: SessionState;
  /** Best first. */
  bids: QueueLevel[];
  asks: QueueLevel[];
}

export interface SecondStats {
  /** Unix seconds. */
  t: number;
  orders: number;
  rejected: number;
  trades: number;
  p50_us: number | null;
  p99_us: number | null;
}

/** `GET /v1/stats`. */
export interface Stats {
  uptime_secs: number;
  orders: number;
  rejected: number;
  trades: number;
  cancels: number;
  /** The last 60 seconds, oldest first. */
  seconds: SecondStats[];
  p50_us: number | null;
  p99_us: number | null;
}

export interface BurstResult {
  symbol: string;
  submitted: number;
  accepted: number;
  rejected: number;
  trades: number;
  elapsed_ms: number;
  orders_per_sec: number;
  p50_us: number;
  p99_us: number;
  cancelled: number;
}

export interface LeaderboardEntry {
  rank: number;
  display_name: string;
  kind: "guest" | "bot";
  pnl: DecimalString;
  you: boolean;
}

export interface LeaderboardResponse {
  entries: LeaderboardEntry[];
}

// ── WebSocket `/v1/stream` events ────────────────────────────────────────────

export interface TradeEvent extends Trade {
  event: "trade";
  buyer_client_id: string;
  seller_client_id: string;
}

/**
 * Private: sent only to the order's owner. `remaining` already counts the
 * order's immediate fills, whose `trade` events arrive before this one.
 */
export interface OrderAcceptedEvent {
  event: "order_accepted";
  order_id: string;
  symbol: string;
  client_id: string;
  side: Side;
  order_type: OrderType;
  quantity: DecimalString;
  time_in_force: TimeInForce;
  remaining: DecimalString;
  status: OrderStatus;
}

/** Private: sent only to the order's owner. */
export interface OrderCancelledEvent {
  event: "order_cancelled";
  order_id: string;
  symbol: string;
  client_id: string;
  side: Side;
  remaining: DecimalString;
}

/** Top 10 levels per side after a change. Coalesced: treat each as a full snapshot. */
export interface BookUpdateEvent {
  event: "book_update";
  symbol: string;
  bids: BookLevel[];
  asks: BookLevel[];
}

export type Activity = "rested" | "filled" | "expired" | "killed" | "cancelled";

/** Public: what the engine did with one order. */
export interface OrderActivityEvent {
  event: "order_activity";
  order_id: string;
  symbol: string;
  participant: string;
  side: Side;
  /** Absent for a market order. */
  price?: DecimalString;
  quantity: DecimalString;
  remaining: DecimalString;
  /** Trades it took part in on arrival. */
  fills: number;
  action: Activity;
}

/** Public: an opening auction's result. */
export interface AuctionEvent {
  event: "auction";
  symbol: string;
  price: DecimalString;
  volume: DecimalString;
  trades: number;
}

/** Private: the visitor's sandbox was removed. */
export interface SandboxRemovedEvent {
  event: "sandbox_removed";
  symbol: string;
  client_id: string;
  reason: "deleted" | "idle" | string;
}

/** Books, positions and history were cleared; every symbol is back in pre-market. */
export interface MarketResetEvent {
  event: "market_reset";
}

export interface SessionStateEvent {
  event: "session_state";
  symbol: string;
  state: string;
}

export type StreamEvent =
  | TradeEvent
  | OrderAcceptedEvent
  | OrderCancelledEvent
  | SessionStateEvent
  | BookUpdateEvent
  | SandboxRemovedEvent
  | OrderActivityEvent
  | AuctionEvent
  | MarketResetEvent;
