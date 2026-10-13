import { component$, useContext } from "@builder.io/qwik";
import { formatMoney } from "~/lib/format";
import { MarketContext } from "~/state/market";

/** Top participants by P&L since the last reset. */
export const Leaderboard = component$(() => {
  const market = useContext(MarketContext);
  const entries = market.leaderboard;

  return (
    <section class="panel leaderboard" aria-labelledby="leaders-title">
      <div class="panel-head">
        <h2 id="leaders-title">Leaderboard</h2>
        <span class="muted small">By P&amp;L, top 10</span>
      </div>
      {entries.length === 0 ? (
        <p class="empty">
          Nobody has traded since the last reset. The first fill puts you on the
          board.
        </p>
      ) : (
        <table>
          <thead>
            <tr>
              <th scope="col">Rank</th>
              <th scope="col">Trader</th>
              <th scope="col">P&amp;L</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => {
              const pnl = Number(e.pnl);
              return (
                <tr
                  key={`${e.rank}-${e.display_name}`}
                  class={[e.you && "mine"]}
                >
                  <td class="num">{e.rank}</td>
                  <td>
                    {e.display_name}
                    {e.kind === "bot" && <span class="tag">Bot</span>}
                    {e.you && <span class="you">You</span>}
                  </td>
                  <td
                    class={[
                      "num",
                      pnl > 0.004 && "tick-up",
                      pnl < -0.004 && "tick-down",
                    ]}
                  >
                    {formatMoney(e.pnl)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
});
