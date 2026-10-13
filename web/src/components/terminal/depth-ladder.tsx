import { component$, useContext } from "@builder.io/qwik";
import type { BookLevel } from "~/api/types";
import { formatPrice, formatQty } from "~/lib/format";
import { MarketContext } from "~/state/market";

const LEVELS = 10;

interface DepthLadderProps {
  /** Defaults to the selected symbol. */
  symbol?: string;
}

export const DepthLadder = component$<DepthLadderProps>((props) => {
  const market = useContext(MarketContext);
  const symbol = props.symbol ?? market.selected;
  const book = market.books[symbol] ?? { asks: [], bids: [], loaded: false };

  // Worst ask at the top so the best prices meet at the spread row.
  const asks = book.asks.slice(0, LEVELS).reverse();
  const bids = book.bids.slice(0, LEVELS);
  const maxQty = Math.max(
    1,
    ...asks.map((l) => Number(l[1]) || 0),
    ...bids.map((l) => Number(l[1]) || 0),
  );
  const bestAsk = book.asks[0]?.[0];
  const bestBid = book.bids[0]?.[0];
  const spread =
    bestAsk && bestBid ? (Number(bestAsk) - Number(bestBid)).toFixed(2) : null;

  const row = (level: BookLevel, side: "ask" | "bid") => {
    const [price, qty] = level;
    const pct = Math.round(((Number(qty) || 0) / maxQty) * 100);
    return (
      <tr key={`${side}-${price}`} class={`level ${side}`}>
        <td>
          <button
            type="button"
            class="price-btn"
            aria-label={`${side === "ask" ? "Ask" : "Bid"} ${formatPrice(price)}, use this price`}
            onClick$={() => {
              market.pickedPrice = formatPrice(price).replace(/,/g, "");
              market.pickedAt = Date.now();
            }}
          >
            {formatPrice(price)}
          </button>
        </td>
        <td class="num">
          <span class="bar" style={{ width: `${pct}%` }} aria-hidden="true" />
          <span>{formatQty(qty)}</span>
        </td>
      </tr>
    );
  };

  const empty = book.loaded && asks.length === 0 && bids.length === 0;

  return (
    <section class="panel ladder" aria-labelledby={`ladder-title-${symbol}`}>
      <div class="panel-head">
        <h2 id={`ladder-title-${symbol}`}>Order book {symbol}</h2>
      </div>
      <table>
        <caption class="visually-hidden">
          Resting orders for {symbol}: asks above, bids below. Select a price to
          copy it into the order ticket.
        </caption>
        <thead>
          <tr>
            <th scope="col">Price</th>
            <th scope="col">Size</th>
          </tr>
        </thead>
        <tbody>
          {!book.loaded ? (
            <tr class="empty">
              <td colSpan={2}>Loading the order book.</td>
            </tr>
          ) : empty ? (
            <tr class="empty">
              <td colSpan={2}>
                No resting orders for {symbol}. Place a limit order to start the
                book.
              </td>
            </tr>
          ) : (
            <>
              {asks.length === 0 && (
                <tr class="empty">
                  <td colSpan={2}>No asks.</td>
                </tr>
              )}
              {asks.map((l) => row(l, "ask"))}
              <tr class="spread">
                <td colSpan={2}>
                  {spread !== null && Number(spread) <= 0 ? (
                    <>
                      Crossed by{" "}
                      <span class="num">
                        {Math.abs(Number(spread)).toFixed(2)}
                      </span>
                      . The opening auction matches these.
                    </>
                  ) : spread !== null ? (
                    <>
                      Spread <span class="num">{spread}</span>
                    </>
                  ) : (
                    "One-sided book"
                  )}
                </td>
              </tr>
              {bids.map((l) => row(l, "bid"))}
              {bids.length === 0 && (
                <tr class="empty">
                  <td colSpan={2}>No bids.</td>
                </tr>
              )}
            </>
          )}
        </tbody>
      </table>
    </section>
  );
});
