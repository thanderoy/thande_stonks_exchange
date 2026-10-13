/**
 * Wire types for the exchange server's `/v1` API.
 *
 * Hand-written for now. These will be generated from the server's OpenAPI
 * spec (`/api-docs/openapi.json`) with `openapi-typescript` once the session
 * and join endpoints land there; keep the names aligned so the swap is cheap.
 *
 * Decimals (prices and quantities) travel as JSON strings and stay strings
 * here. Never parse them into floats for anything that goes back to the server.
 */

export const SYMBOLS = ["AAPL", "MSFT", "GOOGL"] as const;
export type Ticker = (typeof SYMBOLS)[number];

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

export interface SubmitOrderResponse {
  order_id: string;
  trades: Trade[];
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

// ── WebSocket `/v1/stream` events ────────────────────────────────────────────

export interface TradeEvent extends Trade {
  event: "trade";
  buyer_client_id: string;
  seller_client_id: string;
}

export interface OrderAcceptedEvent {
  event: "order_accepted";
  order_id: string;
  symbol: string;
}

export interface OrderCancelledEvent {
  event: "order_cancelled";
  order_id: string;
  symbol: string;
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
  | SessionStateEvent;
