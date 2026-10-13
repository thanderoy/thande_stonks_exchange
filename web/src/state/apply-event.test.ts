import { describe, expect, it } from "vitest";
import type {
  OrderAcceptedEvent,
  SymbolSummary as WireSummary,
  Trade,
  TradeEvent,
} from "~/api/types";
import {
  addPlacedOrder,
  addPoint,
  applySnapshot,
  applyStreamEvent,
  markedPnl,
  subtractDecimal,
  type Snapshot,
} from "./apply-event";
import { TAPE_LIMIT, initialMarketState, type PricePoint } from "./market";

function trade(
  id: string,
  price: string,
  extra: Partial<TradeEvent> = {},
): TradeEvent {
  return {
    event: "trade",
    id,
    symbol: "AAPL",
    price,
    quantity: "10",
    buyer_order_id: "b",
    seller_order_id: "s",
    buyer_client_id: "someone",
    seller_client_id: "other",
    executed_at: "2026-10-12T10:00:00Z",
    ...extra,
  };
}

function accepted(
  orderId: string,
  remaining: string,
  extra: Partial<OrderAcceptedEvent> = {},
): OrderAcceptedEvent {
  return {
    event: "order_accepted",
    order_id: orderId,
    symbol: "AAPL",
    client_id: "me",
    side: "Buy",
    order_type: { type: "Limit", price: "100.00" },
    quantity: "10",
    time_in_force: "GoodTillCanceled",
    remaining,
    status: "resting",
    ...extra,
  };
}

describe("applyStreamEvent", () => {
  it("updates price stats, tick and tape on a trade", () => {
    const state = initialMarketState();
    applyStreamEvent(state, trade("t1", "100.00"), "me");
    applyStreamEvent(state, trade("t2", "100.50", { quantity: "5" }), "me");
    applyStreamEvent(state, trade("t3", "99.50"), "me");

    const s = state.symbols.AAPL;
    expect([s.last, s.open, s.high, s.low]).toEqual([
      "99.50",
      "100.00",
      "100.50",
      "99.50",
    ]);
    expect([s.volume, s.trades, s.tick]).toEqual([25, 3, "down"]);
    expect(state.tape.map((t) => t.id)).toEqual(["t3", "t2", "t1"]);
    expect(state.tape[0].mine).toBeNull();
  });

  it("marks the visitor's own trades and asks for an account reload", () => {
    const state = initialMarketState();
    expect(
      applyStreamEvent(
        state,
        trade("t1", "100", { buyer_client_id: "me" }),
        "me",
      ),
    ).toEqual({ account: true });
    applyStreamEvent(
      state,
      trade("t2", "100", { seller_client_id: "me" }),
      "me",
    );
    expect(applyStreamEvent(state, trade("t3", "100"), "me")).toEqual({});

    expect(state.tape.map((t) => t.mine)).toEqual([null, "sold", "bought"]);
  });

  it("caps the tape length", () => {
    const state = initialMarketState();
    for (let i = 0; i < TAPE_LIMIT + 5; i++)
      applyStreamEvent(state, trade(`t${i}`, "100"), "");
    expect(state.tape).toHaveLength(TAPE_LIMIT);
    expect(state.tape[0].id).toBe(`t${TAPE_LIMIT + 4}`);
  });

  it("keeps one chart point per second", () => {
    const state = initialMarketState();
    applyStreamEvent(
      state,
      trade("t1", "100", { executed_at: "2026-10-12T10:00:00.100Z" }),
      "",
    );
    applyStreamEvent(
      state,
      trade("t2", "101", { executed_at: "2026-10-12T10:00:00.900Z" }),
      "",
    );
    applyStreamEvent(
      state,
      trade("t3", "102", { executed_at: "2026-10-12T10:00:01.000Z" }),
      "",
    );
    const base = Date.parse("2026-10-12T10:00:00Z") / 1000;
    expect(state.series.AAPL).toEqual([
      { time: base, price: 101 },
      { time: base + 1, price: 102 },
    ]);
  });

  it("tracks a resting order through later fills until it is gone", () => {
    const state = initialMarketState();
    // Immediate fill first, then the accepted event that already counts it.
    applyStreamEvent(
      state,
      trade("t1", "100", { buyer_order_id: "o1", quantity: "4" }),
      "me",
    );
    applyStreamEvent(state, accepted("o1", "6"), "me");
    expect(state.myOrders.map((o) => o.remaining)).toEqual(["6"]);

    applyStreamEvent(
      state,
      trade("t2", "100", { buyer_order_id: "o1", quantity: "2.5" }),
      "me",
    );
    expect(state.myOrders[0].remaining).toBe("3.5");
    applyStreamEvent(
      state,
      trade("t3", "100", { buyer_order_id: "o1", quantity: "3.5" }),
      "me",
    );
    expect(state.myOrders).toEqual([]);
  });

  it("does not count trades against an order the stream hasn't confirmed", () => {
    const state = initialMarketState();
    addPlacedOrder(state, {
      orderId: "o1",
      symbol: "AAPL",
      side: "Buy",
      price: "100",
      remaining: "6",
      status: "resting",
      confirmed: false,
    });
    // Could be one of the order's own immediate fills.
    applyStreamEvent(
      state,
      trade("t1", "100", { buyer_order_id: "o1", quantity: "4" }),
      "me",
    );
    expect(state.myOrders[0].remaining).toBe("6");
    applyStreamEvent(state, accepted("o1", "6"), "me");
    expect(state.myOrders[0].confirmed).toBe(true);
  });

  it("lets the stream's copy of an order win over the HTTP response", () => {
    const state = initialMarketState();
    applyStreamEvent(state, accepted("o1", "6"), "me");
    applyStreamEvent(
      state,
      trade("t1", "100", { buyer_order_id: "o1", quantity: "1" }),
      "me",
    );
    addPlacedOrder(state, {
      orderId: "o1",
      symbol: "AAPL",
      side: "Buy",
      price: "100",
      remaining: "6",
      status: "resting",
      confirmed: false,
    });
    expect(state.myOrders).toHaveLength(1);
    expect(state.myOrders[0].remaining).toBe("5");
  });

  it("drops filled, expired and cancelled orders", () => {
    const state = initialMarketState();
    applyStreamEvent(state, accepted("o1", "0", { status: "filled" }), "me");
    applyStreamEvent(state, accepted("o2", "3", { status: "expired" }), "me");
    expect(state.myOrders).toEqual([]);

    applyStreamEvent(state, accepted("o3", "10"), "me");
    applyStreamEvent(
      state,
      {
        event: "order_cancelled",
        order_id: "o3",
        symbol: "AAPL",
        client_id: "me",
        side: "Buy",
        remaining: "10",
      },
      "me",
    );
    expect(state.myOrders).toEqual([]);
  });

  it("ignores someone else's order events", () => {
    const state = initialMarketState();
    applyStreamEvent(state, accepted("o1", "10", { client_id: "other" }), "me");
    expect(state.myOrders).toEqual([]);
  });

  it("replaces the book on book_update and the session on session_state", () => {
    const state = initialMarketState();
    applyStreamEvent(
      state,
      { event: "book_update", symbol: "MSFT", bids: [["99", "5"]], asks: [] },
      "",
    );
    applyStreamEvent(
      state,
      { event: "session_state", symbol: "MSFT", state: "Halted" },
      "",
    );
    expect(state.books.MSFT).toEqual({
      bids: [["99", "5"]],
      asks: [],
      loaded: true,
    });
    expect(state.symbols.MSFT.session).toBe("Halted");
  });

  it("clears the market on market_reset and asks for a resync", () => {
    const state = initialMarketState();
    applyStreamEvent(
      state,
      trade("t1", "100", { buyer_client_id: "me" }),
      "me",
    );
    applyStreamEvent(state, accepted("o1", "10"), "me");
    state.account = {
      client_id: "me",
      display_name: "Me",
      cash: "-1000",
      positions: [
        { symbol: "AAPL", quantity: "10", mark: "100", value: "1000" },
      ],
      pnl: "0",
    };

    expect(applyStreamEvent(state, { event: "market_reset" }, "me")).toEqual({
      resync: true,
      account: true,
    });
    expect(state.tape).toEqual([]);
    expect(state.myOrders).toEqual([]);
    expect(state.series.AAPL).toEqual([]);
    expect(state.symbols.AAPL.last).toBeNull();
    expect(state.account.positions).toEqual([]);
    expect(state.resetAt).toBeGreaterThan(0);
  });
});

function summary(symbol: string, last: string | null): WireSummary {
  return {
    symbol,
    state: "Open",
    open: last,
    last,
    high: last,
    low: last,
    volume: "30",
    trades: 2,
    reference_price: "100",
    collar_low: "80",
    collar_high: "120",
  };
}

function wireTrade(
  id: string,
  price: string,
  at: string,
  extra: Partial<Trade> = {},
): Trade {
  return {
    id,
    symbol: "AAPL",
    price,
    quantity: "10",
    buyer_order_id: "b",
    seller_order_id: "s",
    buyer_client_id: "x",
    seller_client_id: "y",
    executed_at: at,
    ...extra,
  };
}

describe("applySnapshot", () => {
  it("loads stats, books, tape, chart, orders and account", () => {
    const state = initialMarketState();
    const snap: Snapshot = {
      symbols: [summary("AAPL", "101"), summary("MSFT", null)],
      books: [{ symbol: "AAPL", bids: [["100", "1"]], asks: [["101", "2"]] }],
      trades: {
        // Newest first, as the server sends them.
        AAPL: [
          wireTrade("t2", "101", "2026-10-12T10:00:02Z", {
            seller_client_id: "me",
          }),
          wireTrade("t1", "100", "2026-10-12T10:00:01Z"),
        ],
      },
      orders: [
        {
          order_id: "o1",
          symbol: "AAPL",
          side: "Sell",
          order_type: { type: "Limit", price: "105" },
          time_in_force: "GoodTillCanceled",
          remaining: "7",
          submitted_at: "2026-10-12T10:00:00Z",
        },
      ],
      account: {
        client_id: "me",
        display_name: "Me",
        cash: "1010",
        positions: [
          { symbol: "AAPL", quantity: "-10", mark: "101", value: "-1010" },
        ],
        pnl: "0",
      },
    };
    applySnapshot(state, snap, "me");

    expect(state.symbols.AAPL).toMatchObject({
      last: "101",
      session: "Open",
      volume: 30,
      collarLow: "80",
      tick: "up",
    });
    expect(state.symbols.MSFT.last).toBeNull();
    expect(state.books.AAPL.loaded).toBe(true);
    expect(state.tape.map((t) => [t.id, t.tick, t.mine])).toEqual([
      ["t2", "up", "sold"],
      ["t1", "flat", null],
    ]);
    expect(state.series.AAPL.map((p) => p.price)).toEqual([100, 101]);
    expect(state.myOrders).toEqual([
      {
        orderId: "o1",
        symbol: "AAPL",
        side: "Sell",
        price: "105",
        remaining: "7",
        status: "resting",
        confirmed: true,
      },
    ]);
    expect(markedPnl(state)).toBe(0);

    // A later trade moves the mark, and the P&L with it.
    applyStreamEvent(state, trade("t3", "100"), "me");
    expect(markedPnl(state)).toBe(10);
  });
});

describe("helpers", () => {
  it("subtracts decimal strings exactly", () => {
    expect(subtractDecimal("10", "4")).toBe("6");
    expect(subtractDecimal("0.3", "0.1")).toBe("0.2");
    expect(subtractDecimal("100.50", "0.5")).toBe("100");
    expect(subtractDecimal("1", "1.000")).toBe("0");
  });

  it("ignores out-of-order and unparseable chart points", () => {
    const points: PricePoint[] = [];
    addPoint(points, "2026-10-12T10:00:05Z", "100");
    addPoint(points, "2026-10-12T10:00:04Z", "99");
    addPoint(points, "garbage", "99");
    addPoint(points, "2026-10-12T10:00:06Z", "abc");
    expect(points).toHaveLength(1);
  });
});
