import { createContextId } from "@builder.io/qwik";
import {
  SYMBOLS,
  type Account,
  type BookLevel,
  type LeaderboardEntry,
  type Sandbox,
  type SessionState,
  type Side,
  type SymbolKind,
} from "~/api/types";
import type { StreamStatus } from "~/api/stream";
import type { Tick } from "~/lib/format";

export interface SymbolSummary {
  kind: SymbolKind;
  last: string | null;
  tick: Tick;
  session: SessionState | string | null;
  /** First trade since the last reset. */
  open: string | null;
  high: string | null;
  low: string | null;
  volume: number;
  trades: number;
  collarLow: string | null;
  collarHigh: string | null;
}

export interface BookState {
  asks: BookLevel[];
  bids: BookLevel[];
  loaded: boolean;
}

export interface TapeEntry {
  id: string;
  symbol: string;
  price: string;
  quantity: string;
  executedAt: string;
  tick: Tick;
  /** Set when the signed-in guest was on either side of the trade. */
  mine: "bought" | "sold" | null;
}

/** One chart point: the last trade price in a given second. */
export interface PricePoint {
  /** Unix seconds. */
  time: number;
  price: number;
}

/** One of the visitor's resting orders. */
export interface MyOrder {
  orderId: string;
  symbol: string;
  side: Side;
  /** Limit price; null for an order with no limit. */
  price: string | null;
  remaining: string;
  status: "resting" | "cancelling";
  /**
   * True once `remaining` comes from the server's view (the `order_accepted`
   * event or `GET /v1/me/orders`). Only confirmed orders count later trades
   * against them; before that, a trade may be one of the order's immediate
   * fills, which `remaining` already includes.
   */
  confirmed: boolean;
}

export interface MarketState {
  selected: string;
  /** Symbols to show, in order: the shared ones, then the visitor's sandbox. */
  order: string[];
  /** The visitor's sandbox, or null. */
  sandbox: Sandbox | null;
  symbols: Record<string, SymbolSummary>;
  books: Record<string, BookState>;
  tape: TapeEntry[];
  /** Chart points per symbol, oldest first. */
  series: Record<string, PricePoint[]>;
  myOrders: MyOrder[];
  /** Null until loaded. */
  account: Account | null;
  leaderboard: LeaderboardEntry[];
  stream: StreamStatus;
  /** Price clicked in the depth ladder; the order ticket copies it. */
  pickedPrice: string;
  pickedAt: number;
  /** When the last `market_reset` arrived (ms), so pages can say so. 0 = none seen. */
  resetAt: number;
  /** Bumped whenever the visitor's account needs reloading from the server. */
  accountVersion: number;
}

export const TAPE_LIMIT = 60;
export const SERIES_LIMIT = 1000;

export function emptySummary(kind: SymbolKind = "shared"): SymbolSummary {
  return {
    kind,
    last: null,
    tick: "flat",
    session: null,
    open: null,
    high: null,
    low: null,
    volume: 0,
    trades: 0,
    collarLow: null,
    collarHigh: null,
  };
}

export function emptyBook(loaded = false): BookState {
  return { asks: [], bids: [], loaded };
}

export function initialMarketState(): MarketState {
  const symbols: Record<string, SymbolSummary> = {};
  const books: Record<string, BookState> = {};
  const series: Record<string, PricePoint[]> = {};
  for (const s of SYMBOLS) {
    symbols[s] = emptySummary();
    books[s] = emptyBook();
    series[s] = [];
  }
  return {
    selected: SYMBOLS[0],
    order: [...SYMBOLS],
    sandbox: null,
    symbols,
    books,
    tape: [],
    series,
    myOrders: [],
    account: null,
    leaderboard: [],
    stream: "connecting",
    pickedPrice: "",
    pickedAt: 0,
    resetAt: 0,
    accountVersion: 0,
  };
}

export const MarketContext = createContextId<MarketState>("tse.market");
