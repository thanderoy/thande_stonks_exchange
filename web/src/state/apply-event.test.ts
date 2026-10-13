import { describe, expect, it } from "vitest";
import type { TradeEvent } from "~/api/types";
import { applyStreamEvent } from "./apply-event";
import { TAPE_LIMIT, initialMarketState } from "./market";

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

describe("applyStreamEvent", () => {
  it("updates last price, tick and tape on a trade", () => {
    const state = initialMarketState();
    applyStreamEvent(state, trade("t1", "100.00"), "me");
    applyStreamEvent(state, trade("t2", "100.50"), "me");

    expect(state.symbols.AAPL.last).toBe("100.50");
    expect(state.symbols.AAPL.tick).toBe("up");
    expect(state.tape.map((t) => t.id)).toEqual(["t2", "t1"]);
    expect(state.tape[0].mine).toBeNull();
  });

  it("marks the visitor's own trades", () => {
    const state = initialMarketState();
    applyStreamEvent(
      state,
      trade("t1", "100", { buyer_client_id: "me" }),
      "me",
    );
    applyStreamEvent(
      state,
      trade("t2", "100", { seller_client_id: "me" }),
      "me",
    );

    expect(state.tape.map((t) => t.mine)).toEqual(["sold", "bought"]);
  });

  it("caps the tape length", () => {
    const state = initialMarketState();
    for (let i = 0; i < TAPE_LIMIT + 5; i++)
      applyStreamEvent(state, trade(`t${i}`, "100"), "");
    expect(state.tape).toHaveLength(TAPE_LIMIT);
    expect(state.tape[0].id).toBe(`t${TAPE_LIMIT + 4}`);
  });

  it("applies session changes and cancels", () => {
    const state = initialMarketState();
    state.myOrders = [
      {
        orderId: "o1",
        symbol: "MSFT",
        side: "Buy",
        summary: "",
        status: "working",
      },
    ];
    applyStreamEvent(
      state,
      { event: "session_state", symbol: "MSFT", state: "Halted" },
      "",
    );
    applyStreamEvent(
      state,
      { event: "order_cancelled", order_id: "o1", symbol: "MSFT" },
      "",
    );

    expect(state.symbols.MSFT.session).toBe("Halted");
    expect(state.myOrders[0].status).toBe("cancelled");
  });
});
