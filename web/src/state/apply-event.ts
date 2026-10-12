import type { StreamEvent } from "~/api/types";
import { tickDirection } from "~/lib/format";
import { TAPE_LIMIT, type MarketState, type TapeEntry } from "./market";

/**
 * Fold one stream event into the market state. Kept free of Qwik and the DOM
 * so it can be unit tested; the trade page calls it with its store.
 */
export function applyStreamEvent(
  state: MarketState,
  ev: StreamEvent,
  myClientId: string,
): void {
  switch (ev.event) {
    case "trade": {
      const summary = state.symbols[ev.symbol] ?? {
        last: null,
        tick: "flat",
        session: null,
      };
      const tick = tickDirection(summary.last, ev.price);
      summary.tick = tick === "flat" ? summary.tick : tick;
      summary.last = ev.price;
      state.symbols[ev.symbol] = summary;

      let mine: TapeEntry["mine"] = null;
      if (myClientId && ev.buyer_client_id === myClientId) mine = "bought";
      else if (myClientId && ev.seller_client_id === myClientId) mine = "sold";
      state.tape = [
        {
          id: ev.id,
          symbol: ev.symbol,
          price: ev.price,
          quantity: ev.quantity,
          executedAt: ev.executed_at,
          tick,
          mine,
        },
        ...state.tape,
      ].slice(0, TAPE_LIMIT);
      return;
    }
    case "session_state": {
      const summary = state.symbols[ev.symbol];
      if (summary) summary.session = ev.state;
      return;
    }
    case "order_cancelled": {
      const order = state.myOrders.find((o) => o.orderId === ev.order_id);
      if (order) order.status = "cancelled";
      return;
    }
    case "order_accepted":
      // Not shown yet; the order ticket already reports the visitor's own orders.
      return;
  }
}
