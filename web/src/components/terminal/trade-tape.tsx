import { component$, useContext } from "@builder.io/qwik";
import { formatPrice, formatQty, formatTime } from "~/lib/format";
import { MarketContext } from "~/state/market";

export const TradeTape = component$(() => {
  const market = useContext(MarketContext);

  return (
    <section class="panel tape" aria-labelledby="tape-title">
      <div class="panel-head">
        <h2 id="tape-title">Trades</h2>
        <span class="muted small">All symbols, newest first</span>
      </div>
      {market.tape.length === 0 ? (
        <p class="empty">
          No trades since you opened this page. Trades appear here the moment
          they execute.
        </p>
      ) : (
        <div class="tape-scroll" tabIndex={0} aria-label="Trade list">
          <table>
            <thead>
              <tr>
                <th scope="col">Time</th>
                <th scope="col">Symbol</th>
                <th scope="col">Price</th>
                <th scope="col">Size</th>
              </tr>
            </thead>
            <tbody>
              {market.tape.map((t) => (
                <tr key={t.id} class={["fresh", t.mine && "mine"]}>
                  <td class="num muted">{formatTime(t.executedAt)}</td>
                  <td>
                    {t.symbol}
                    {t.mine && (
                      <span class="you">
                        {t.mine === "bought" ? "You bought" : "You sold"}
                      </span>
                    )}
                  </td>
                  <td
                    class={[
                      "num",
                      t.tick === "up" && "tick-up",
                      t.tick === "down" && "tick-down",
                    ]}
                  >
                    <span aria-hidden="true">
                      {t.tick === "up" ? "▲ " : t.tick === "down" ? "▼ " : ""}
                    </span>
                    {formatPrice(t.price)}
                  </td>
                  <td class="num">{formatQty(t.quantity)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
});
