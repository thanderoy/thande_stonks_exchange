import type {
  Account,
  BookSnapshot,
  OpenOrder,
  Sandbox,
  StreamEvent,
  SymbolSummary as WireSummary,
  Trade,
} from "~/api/types";
import { tickDirection } from "~/lib/format";
import {
  ACTIVITY_LIMIT,
  SERIES_LIMIT,
  TAPE_LIMIT,
  emptyBook,
  emptySummary,
  type MarketState,
  type MyOrder,
  type PricePoint,
  type SymbolSummary,
  type TapeEntry,
  type ActivityEntry,
} from "./market";

/** Follow-up work a caller should do after folding in an event. */
export interface Effects {
  /** The visitor traded: reload their account. */
  account?: boolean;
  /** Everything was reset: reload every snapshot. */
  resync?: boolean;
}

/**
 * Fold one stream event into the market state. Kept free of Qwik and the DOM
 * so it can be unit tested; the app layout calls it with its store.
 */
export function applyStreamEvent(
  state: MarketState,
  ev: StreamEvent,
  myClientId: string,
): Effects {
  switch (ev.event) {
    case "trade": {
      const summary = summaryFor(state, ev.symbol);
      const tick = tickDirection(summary.last, ev.price);
      summary.tick = tick === "flat" ? summary.tick : tick;
      recordStats(summary, ev.price, ev.quantity);

      const mine = mySide(ev, myClientId);
      state.tape = [tapeEntry(ev, tick, mine), ...state.tape].slice(
        0,
        TAPE_LIMIT,
      );
      addPoint(seriesFor(state, ev.symbol), ev.executed_at, ev.price);
      fillMyOrders(state, ev);
      return mine ? { account: true } : {};
    }
    case "order_accepted": {
      if (myClientId && ev.client_id !== myClientId) return {};
      const others = state.myOrders.filter((o) => o.orderId !== ev.order_id);
      if (ev.status !== "resting") {
        state.myOrders = others;
        return {};
      }
      const existing = state.myOrders.find((o) => o.orderId === ev.order_id);
      const order: MyOrder = {
        orderId: ev.order_id,
        symbol: ev.symbol,
        side: ev.side,
        price: ev.order_type.type === "Limit" ? ev.order_type.price : null,
        remaining: ev.remaining,
        status: existing?.status ?? "resting",
        confirmed: true,
      };
      state.myOrders = existing
        ? state.myOrders.map((o) => (o.orderId === ev.order_id ? order : o))
        : [order, ...others];
      return {};
    }
    case "order_cancelled": {
      state.myOrders = state.myOrders.filter((o) => o.orderId !== ev.order_id);
      return {};
    }
    case "order_activity": {
      logActivity(state, {
        key: `${ev.order_id}-${ev.action}`,
        symbol: ev.symbol,
        at: Date.now(),
        kind: "order",
        orderId: ev.order_id,
        participant: ev.participant,
        side: ev.side,
        price: ev.price,
        quantity: ev.quantity,
        remaining: ev.remaining,
        fills: ev.fills,
        action: ev.action,
      });
      return {};
    }
    case "auction": {
      logActivity(state, {
        key: `auction-${ev.symbol}-${Date.now()}-${state.activity.length}`,
        symbol: ev.symbol,
        at: Date.now(),
        kind: "auction",
        price: ev.price,
        volume: ev.volume,
        trades: ev.trades,
      });
      return {};
    }
    case "book_update": {
      state.books[ev.symbol] = { asks: ev.asks, bids: ev.bids, loaded: true };
      return {};
    }
    case "session_state": {
      summaryFor(state, ev.symbol).session = ev.state;
      return {};
    }
    case "sandbox_removed": {
      if (myClientId && ev.client_id !== myClientId) return {};
      dropSymbol(state, ev.symbol);
      state.sandbox = null;
      return {};
    }
    case "market_reset": {
      // The reset removed every sandbox.
      for (const symbol of [...state.order]) {
        if (state.symbols[symbol]?.kind === "sandbox")
          dropSymbol(state, symbol);
      }
      state.sandbox = null;
      for (const symbol of Object.keys(state.symbols)) {
        const session = state.symbols[symbol].session;
        const collar = state.symbols[symbol];
        state.symbols[symbol] = {
          ...emptySummary(collar.kind),
          session,
          collarLow: collar.collarLow,
          collarHigh: collar.collarHigh,
        };
        state.books[symbol] = emptyBook(true);
        state.series[symbol] = [];
      }
      state.tape = [];
      state.activity = [];
      state.myOrders = [];
      if (state.account) {
        state.account = {
          ...state.account,
          cash: "0",
          positions: [],
          pnl: "0",
        };
      }
      state.resetAt = Date.now();
      return { resync: true, account: true };
    }
  }
}

/** Everything a page needs on first load or after a reconnect. */
export interface Snapshot {
  symbols: WireSummary[];
  books: BookSnapshot[];
  /** Recent trades per symbol, newest first, as the server sends them. */
  trades: Record<string, Trade[]>;
  /** Null when there is no guest session. */
  orders: OpenOrder[] | null;
  account: Account | null;
  /** The visitor's sandbox; null when they have none (or no session). */
  sandbox: Sandbox | null;
}

/** Replace the state with a fresh snapshot from the REST endpoints. */
export function applySnapshot(
  state: MarketState,
  snap: Snapshot,
  myClientId: string,
): void {
  const listed = snap.symbols.map((s) => s.symbol);
  for (const symbol of [...state.order]) {
    if (!listed.includes(symbol)) dropSymbol(state, symbol);
  }
  state.order = listed;
  state.sandbox = snap.sandbox;
  if (!listed.includes(state.selected) && listed.length > 0) {
    state.selected = listed[0];
  }
  for (const s of snap.symbols) {
    const previous = state.symbols[s.symbol] ?? emptySummary(s.kind);
    state.symbols[s.symbol] = {
      kind: s.kind,
      last: s.last,
      tick: previous.tick,
      session: s.state,
      open: s.open,
      high: s.high,
      low: s.low,
      volume: Number(s.volume) || 0,
      trades: s.trades,
      collarLow: s.collar_low,
      collarHigh: s.collar_high,
    };
  }
  for (const b of snap.books) {
    state.books[b.symbol] = { asks: b.asks, bids: b.bids, loaded: true };
  }

  const tape: TapeEntry[] = [];
  for (const [symbol, trades] of Object.entries(snap.trades)) {
    const points: PricePoint[] = [];
    let lastTick: TapeEntry["tick"] = "flat";
    // Oldest first, so each tick compares with the trade before it.
    const ordered = [...trades].reverse();
    ordered.forEach((t, i) => {
      const tick = tickDirection(ordered[i - 1]?.price, t.price);
      if (tick !== "flat") lastTick = tick;
      tape.push(tapeEntry(t, tick, mySide(t, myClientId)));
      addPoint(points, t.executed_at, t.price);
    });
    state.series[symbol] = points;
    if (state.symbols[symbol]) state.symbols[symbol].tick = lastTick;
  }
  tape.sort((a, b) => b.executedAt.localeCompare(a.executedAt));
  state.tape = tape.slice(0, TAPE_LIMIT);

  if (snap.orders) {
    state.myOrders = snap.orders.map((o) => ({
      orderId: o.order_id,
      symbol: o.symbol,
      side: o.side,
      price: o.order_type.type === "Limit" ? o.order_type.price : null,
      remaining: o.remaining,
      status: "resting",
      confirmed: true,
    }));
  }
  if (snap.account) state.account = snap.account;
}

/**
 * Add an order the visitor just placed, from the HTTP response. If the stream
 * already reported it, the stream's copy is newer, so it wins.
 */
export function addPlacedOrder(state: MarketState, order: MyOrder): void {
  if (state.myOrders.some((o) => o.orderId === order.orderId)) return;
  state.myOrders = [order, ...state.myOrders];
}

/**
 * Cash plus positions at the latest trade price. For display only: the
 * server's `/v1/me/account` is the exact figure, and this tracks it between
 * reloads as prices move.
 */
export function markedPnl(state: MarketState): number | null {
  const account = state.account;
  if (!account) return null;
  let pnl = Number(account.cash);
  for (const p of account.positions) {
    const last = state.symbols[p.symbol]?.last ?? p.mark;
    pnl += Number(p.quantity) * Number(last);
  }
  return pnl;
}

/** Forget a symbol that no longer exists (a removed sandbox). */
function dropSymbol(state: MarketState, symbol: string) {
  state.order = state.order.filter((s) => s !== symbol);
  delete state.symbols[symbol];
  delete state.books[symbol];
  delete state.series[symbol];
  state.tape = state.tape.filter((t) => t.symbol !== symbol);
  state.activity = state.activity.filter((a) => a.symbol !== symbol);
  state.myOrders = state.myOrders.filter((o) => o.symbol !== symbol);
  if (state.selected === symbol && state.order.length > 0) {
    state.selected = state.order[0];
  }
}

function logActivity(state: MarketState, entry: ActivityEntry) {
  state.activity = [entry, ...state.activity].slice(0, ACTIVITY_LIMIT);
}

function summaryFor(state: MarketState, symbol: string): SymbolSummary {
  if (!state.symbols[symbol]) state.symbols[symbol] = emptySummary();
  return state.symbols[symbol];
}

function seriesFor(state: MarketState, symbol: string): PricePoint[] {
  if (!state.series[symbol]) state.series[symbol] = [];
  return state.series[symbol];
}

function recordStats(s: SymbolSummary, price: string, quantity: string) {
  const p = Number(price);
  s.last = price;
  s.open ??= price;
  if (s.high == null || p > Number(s.high)) s.high = price;
  if (s.low == null || p < Number(s.low)) s.low = price;
  s.volume += Number(quantity) || 0;
  s.trades += 1;
}

function mySide(
  t: Pick<Trade, "buyer_client_id" | "seller_client_id">,
  me: string,
): TapeEntry["mine"] {
  if (!me) return null;
  if (t.buyer_client_id === me) return "bought";
  if (t.seller_client_id === me) return "sold";
  return null;
}

function tapeEntry(
  t: Trade,
  tick: TapeEntry["tick"],
  mine: TapeEntry["mine"],
): TapeEntry {
  return {
    id: t.id,
    symbol: t.symbol,
    price: t.price,
    quantity: t.quantity,
    executedAt: t.executed_at,
    tick,
    mine,
  };
}

/** Append a trade to a chart series, keeping one point per second. */
export function addPoint(
  points: PricePoint[],
  executedAt: string,
  price: string,
) {
  const ms = Date.parse(executedAt);
  const value = Number(price);
  if (!Number.isFinite(ms) || !Number.isFinite(value)) return;
  const time = Math.floor(ms / 1000);
  const last = points[points.length - 1];
  if (last && time < last.time) return;
  if (last && last.time === time) last.price = value;
  else points.push({ time, price: value });
  if (points.length > SERIES_LIMIT)
    points.splice(0, points.length - SERIES_LIMIT);
}

/** Count a later fill against the visitor's confirmed resting orders. */
function fillMyOrders(state: MarketState, t: Trade) {
  let changed = false;
  const next = state.myOrders.flatMap((o) => {
    if (
      !o.confirmed ||
      (o.orderId !== t.buyer_order_id && o.orderId !== t.seller_order_id)
    ) {
      return [o];
    }
    changed = true;
    const left = subtractDecimal(o.remaining, t.quantity);
    return Number(left) > 0 ? [{ ...o, remaining: left }] : [];
  });
  if (changed) state.myOrders = next;
}

/**
 * `a - b` on decimal strings without float rounding, for quantities (which
 * never go negative here). Falls back to Number for unexpected input.
 */
export function subtractDecimal(a: string, b: string): string {
  const scale = Math.max(fracLen(a), fracLen(b));
  const toInt = (v: string) => {
    const [i, f = ""] = v.split(".");
    return BigInt(i + f.padEnd(scale, "0"));
  };
  if (!/^\d+(\.\d+)?$/.test(a) || !/^\d+(\.\d+)?$/.test(b)) {
    return String(Number(a) - Number(b));
  }
  const diff = toInt(a) - toInt(b);
  const negative = diff < 0n;
  const digits = (negative ? -diff : diff).toString().padStart(scale + 1, "0");
  const int = digits.slice(0, digits.length - scale);
  const frac = scale
    ? digits.slice(digits.length - scale).replace(/0+$/, "")
    : "";
  return `${negative ? "-" : ""}${int}${frac ? `.${frac}` : ""}`;
}

function fracLen(v: string): number {
  const dot = v.indexOf(".");
  return dot < 0 ? 0 : v.length - dot - 1;
}
