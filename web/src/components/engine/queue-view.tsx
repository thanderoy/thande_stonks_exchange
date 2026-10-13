import { component$ } from "@builder.io/qwik";
import type { EngineView, QueueLevel } from "~/api/types";
import { formatPrice, formatQty } from "~/lib/format";

interface QueueViewProps {
  view: EngineView | null;
  error: string;
  explain: boolean;
}

/** Narrowest and widest order block, in pixels. */
const MIN_BLOCK = 72;
const MAX_BLOCK = 220;

/**
 * The book as the engine stores it: one row per price level, each order a
 * block in its queue, front (fills first) on the left, width by size.
 */
export const QueueView = component$<QueueViewProps>(
  ({ view, error, explain }) => {
    if (error)
      return (
        <p class="empty" role="status">
          {error}
        </p>
      );
    if (!view) return <p class="empty">Loading the queues.</p>;

    const all = [...view.asks, ...view.bids].flatMap((l) => l.orders);
    const largest = Math.max(1, ...all.map((o) => Number(o.remaining) || 0));
    const width = (qty: string) =>
      Math.round(
        MIN_BLOCK + ((Number(qty) || 0) / largest) * (MAX_BLOCK - MIN_BLOCK),
      );

    const row = (level: QueueLevel, side: "ask" | "bid") => (
      <li key={`${side}-${level.price}`} class={["qrow", side]}>
        <span class="qprice num">{formatPrice(level.price)}</span>
        <ol
          class="queue"
          aria-label={`${side === "ask" ? "Asks" : "Bids"} at ${formatPrice(level.price)}, ${level.orders.length + level.hidden} orders, front first`}
        >
          {level.orders.map((o, i) => (
            <li
              key={o.order_id}
              class={["qorder", o.you && "you"]}
              style={{ width: `${width(o.remaining)}px` }}
              title={`${i + 1}. ${o.participant}${o.you ? " (you)" : ""}: ${formatQty(o.remaining)} at ${formatPrice(level.price)}`}
            >
              <span class="qwho">{o.you ? "You" : o.participant}</span>
              <span class="qqty num">{formatQty(o.remaining)}</span>
            </li>
          ))}
          {level.hidden > 0 && (
            <li class="qmore muted">+{level.hidden} more</li>
          )}
        </ol>
        <span class="qtotal num muted">{formatQty(level.total)}</span>
      </li>
    );

    const empty = view.asks.length === 0 && view.bids.length === 0;
    const bestAsk = view.asks[0]?.price;
    const bestBid = view.bids[0]?.price;
    const spread =
      bestAsk && bestBid ? Number(bestAsk) - Number(bestBid) : null;

    return (
      <div class="queues">
        {explain && (
          <p class="explain">
            Each block is one resting order, as wide as its size. Within a
            price, the engine fills the leftmost block first: the order that
            arrived first. That is price-time priority.
          </p>
        )}
        {empty ? (
          <p class="empty">
            Nothing is resting on {view.symbol}. Place a limit order to start a
            queue.
          </p>
        ) : (
          <ol class="qlevels">
            {[...view.asks].reverse().map((l) => row(l, "ask"))}
            <li class="qspread muted">
              {spread === null
                ? "One-sided book"
                : spread <= 0
                  ? `Crossed by ${Math.abs(spread).toFixed(2)}: the opening auction matches these`
                  : `Spread ${spread.toFixed(2)}`}
            </li>
            {view.bids.map((l) => row(l, "bid"))}
          </ol>
        )}
        {explain && !empty && (
          <p class="explain">
            A new buy at or above the best ask trades against the asks from the
            bottom row up, left to right. Whatever is left joins the back of its
            own price's queue, unless it is an IOC, FOK or market order.
          </p>
        )}
      </div>
    );
  },
);
