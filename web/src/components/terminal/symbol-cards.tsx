import { component$, useContext } from "@builder.io/qwik";
import { SYMBOLS } from "~/api/types";
import {
  COMPANY_NAMES,
  formatPrice,
  sessionLabel,
  sessionTone,
} from "~/lib/format";
import { MarketContext } from "~/state/market";

export const SymbolCards = component$(() => {
  const market = useContext(MarketContext);

  return (
    <div class="cards" role="group" aria-label="Symbols">
      {SYMBOLS.map((sym) => {
        const s = market.symbols[sym];
        const selected = market.selected === sym;
        return (
          <button
            key={sym}
            type="button"
            class="card"
            aria-pressed={selected ? "true" : "false"}
            onClick$={() => {
              if (market.selected === sym) return;
              market.selected = sym;
              market.book = {
                symbol: sym,
                asks: [],
                bids: [],
                loaded: false,
                error: "",
              };
            }}
          >
            <span class="card-sym">{sym}</span>
            <span class="card-name">{COMPANY_NAMES[sym]}</span>
            {s.last ? (
              <span
                class={[
                  "card-last num",
                  s.tick === "up" && "tick-up",
                  s.tick === "down" && "tick-down",
                ]}
              >
                <span aria-hidden="true">
                  {s.tick === "up" ? "▲ " : s.tick === "down" ? "▼ " : ""}
                </span>
                <span class="visually-hidden">Last trade </span>
                {formatPrice(s.last)}
              </span>
            ) : (
              <span class="card-last none">No trades yet</span>
            )}
            <span class="pill" data-tone={sessionTone(s.session)}>
              {s.session == null ? "Checking" : sessionLabel(s.session)}
            </span>
          </button>
        );
      })}
    </div>
  );
});
