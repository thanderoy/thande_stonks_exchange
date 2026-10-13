import { component$, useContext } from "@builder.io/qwik";
import {
  formatChange,
  formatPrice,
  formatQty,
  sessionLabel,
  sessionTone,
  symbolName,
} from "~/lib/format";
import { MarketContext } from "~/state/market";

/** Every shared symbol's session and the day's numbers, read only. */
export const SessionBoard = component$(() => {
  const market = useContext(MarketContext);

  return (
    <section class="panel sessions" aria-labelledby="sessions-title">
      <div class="panel-head">
        <h2 id="sessions-title">Sessions</h2>
        <span class="muted small">Since the last reset</span>
      </div>
      <div class="table-scroll">
        <table>
          <thead>
            <tr>
              <th scope="col">Symbol</th>
              <th scope="col">Session</th>
              <th scope="col">Last</th>
              <th scope="col">Change</th>
              <th scope="col">High</th>
              <th scope="col">Low</th>
              <th scope="col">Volume</th>
              <th scope="col">Trades</th>
              <th scope="col">Price collar</th>
            </tr>
          </thead>
          <tbody>
            {market.order.map((sym) => {
              const s = market.symbols[sym];
              return (
                <tr key={sym}>
                  <th scope="row">
                    <span class="sym">{sym}</span>
                    <span class="muted small"> {symbolName(sym, s.kind)}</span>
                  </th>
                  <td>
                    <span class="pill" data-tone={sessionTone(s.session)}>
                      {s.session == null ? "Checking" : sessionLabel(s.session)}
                    </span>
                  </td>
                  <td class="num">{s.last ? formatPrice(s.last) : "–"}</td>
                  <td
                    class={[
                      "num",
                      s.last &&
                        s.open &&
                        Number(s.last) > Number(s.open) &&
                        "tick-up",
                      s.last &&
                        s.open &&
                        Number(s.last) < Number(s.open) &&
                        "tick-down",
                    ]}
                  >
                    {s.last && s.open ? formatChange(s.last, s.open) : "–"}
                  </td>
                  <td class="num">{s.high ? formatPrice(s.high) : "–"}</td>
                  <td class="num">{s.low ? formatPrice(s.low) : "–"}</td>
                  <td class="num">{formatQty(String(s.volume))}</td>
                  <td class="num">{formatQty(String(s.trades))}</td>
                  <td class="num">
                    {s.collarLow && s.collarHigh
                      ? `${formatPrice(s.collarLow)}–${formatPrice(s.collarHigh)}`
                      : "–"}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
});
