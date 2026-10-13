import { createContextId } from "@builder.io/qwik";
import {
  SYMBOLS,
  type BookLevel,
  type SessionState,
  type Side,
  type Ticker,
} from "~/api/types";
import type { StreamStatus } from "~/api/stream";
import type { Tick } from "~/lib/format";

export interface SymbolSummary {
  last: string | null;
  tick: Tick;
  session: SessionState | string | null;
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

export interface MyOrder {
  orderId: string;
  symbol: string;
  side: Side;
  summary: string;
  status: "working" | "cancelling" | "cancelled" | "done";
}

export interface MarketState {
  selected: Ticker;
  symbols: Record<string, SymbolSummary>;
  book: {
    symbol: string;
    asks: BookLevel[];
    bids: BookLevel[];
    loaded: boolean;
    error: string;
  };
  tape: TapeEntry[];
  myOrders: MyOrder[];
  stream: StreamStatus;
  /** Price clicked in the depth ladder; the order ticket copies it. */
  pickedPrice: string;
  pickedAt: number;
}

export const TAPE_LIMIT = 60;

export function initialMarketState(): MarketState {
  const symbols: Record<string, SymbolSummary> = {};
  for (const s of SYMBOLS)
    symbols[s] = { last: null, tick: "flat", session: null };
  return {
    selected: SYMBOLS[0],
    symbols,
    book: { symbol: SYMBOLS[0], asks: [], bids: [], loaded: false, error: "" },
    tape: [],
    myOrders: [],
    stream: "connecting",
    pickedPrice: "",
    pickedAt: 0,
  };
}

export const MarketContext = createContextId<MarketState>("tse.market");
