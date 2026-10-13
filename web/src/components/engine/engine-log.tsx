import { component$, useContext } from "@builder.io/qwik";
import { describeActivity } from "~/lib/format";
import { MarketContext } from "~/state/market";

const pad = (n: number) => String(n).padStart(2, "0");
const clock = (ms: number) => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
};

/** What the engine did, newest first, for one symbol. */
export const EngineLog = component$<{ symbol: string; explain: boolean }>(
  ({ symbol, explain }) => {
    const market = useContext(MarketContext);
    const mine = new Set(market.myOrders.map((o) => o.orderId));
    const entries = market.activity
      .filter((a) => a.symbol === symbol)
      .slice(0, 40);

    return (
      <section class="panel engine-log" aria-labelledby="engine-log-title">
        <div class="panel-head">
          <h2 id="engine-log-title">Engine events {symbol}</h2>
          <span class="muted small">Since you opened this page</span>
        </div>
        {explain && (
          <p class="explain">
            Every order gets one line: it rested, filled, expired (IOC or market
            orders drop what they can't fill), was killed (fill-or-kill orders
            trade in full or not at all) or was cancelled.
          </p>
        )}
        {entries.length === 0 ? (
          <p class="empty">
            Waiting for orders on {symbol}. They appear here as the engine
            handles them.
          </p>
        ) : (
          <ol class="log" aria-live="off">
            {entries.map((a) => (
              <li
                key={a.key}
                class={[
                  "log-line",
                  a.kind === "auction" && "auction",
                  a.action && `act-${a.action}`,
                  a.orderId && mine.has(a.orderId) && "mine",
                ]}
              >
                <span class="num muted">{clock(a.at)}</span>
                <span>{describeActivity(a)}</span>
              </li>
            ))}
          </ol>
        )}
      </section>
    );
  },
);
