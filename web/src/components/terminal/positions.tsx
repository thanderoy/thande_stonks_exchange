import { component$, useContext } from "@builder.io/qwik";
import { formatMoney, formatPrice, formatSignedQty } from "~/lib/format";
import { markedPnl } from "~/state/apply-event";
import { MarketContext } from "~/state/market";

/** Cash, positions and P&L, marked at the latest trade price. */
export const Positions = component$(() => {
  const market = useContext(MarketContext);
  const account = market.account;
  const pnl = markedPnl(market);

  return (
    <section class="panel positions" aria-labelledby="positions-title">
      <div class="panel-head">
        <h2 id="positions-title">Your positions</h2>
        {pnl != null && (
          <span
            class={[
              "pnl num",
              pnl > 0.004 && "tick-up",
              pnl < -0.004 && "tick-down",
            ]}
          >
            <span class="visually-hidden">Profit and loss </span>
            {formatMoney(pnl)}
          </span>
        )}
      </div>
      {!account ? (
        <p class="empty">Loading your account.</p>
      ) : account.positions.length === 0 ? (
        <p class="empty">
          No positions. Everyone starts flat with zero cash; buy or sell to open
          one. Short selling is allowed.
        </p>
      ) : (
        <table>
          <caption class="visually-hidden">
            Your positions, valued at the latest trade price
          </caption>
          <thead>
            <tr>
              <th scope="col">Symbol</th>
              <th scope="col">Shares</th>
              <th scope="col">Last</th>
              <th scope="col">Value</th>
            </tr>
          </thead>
          <tbody>
            {account.positions.map((p) => {
              const last = market.symbols[p.symbol]?.last ?? p.mark;
              return (
                <tr key={p.symbol}>
                  <td>{p.symbol}</td>
                  <td class="num">{formatSignedQty(p.quantity)}</td>
                  <td class="num">{formatPrice(last)}</td>
                  <td class="num">
                    {formatMoney(Number(p.quantity) * Number(last))}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {account && (
        <p class="panel-note muted">
          Cash from trades <span class="num">{formatMoney(account.cash)}</span>.
          P&amp;L is cash plus positions at the last price.
        </p>
      )}
    </section>
  );
});
