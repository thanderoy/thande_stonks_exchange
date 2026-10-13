import {
  component$,
  useContext,
  useSignal,
  useVisibleTask$,
} from "@builder.io/qwik";
import type { DocumentHead } from "@builder.io/qwik-city";
import { ApiError, getEngine } from "~/api/client";
import type { EngineView } from "~/api/types";
import { EngineLog } from "~/components/engine/engine-log";
import { QueueView } from "~/components/engine/queue-view";
import { StreamStatus } from "~/components/terminal/stream-status";
import { SymbolCards } from "~/components/terminal/symbol-cards";
import { MarketContext } from "~/state/market";

const EXPLAIN_KEY = "tse-explain";
/** At most one queue reload per this many milliseconds while the book is busy. */
const REFRESH_MS = 300;

export default component$(() => {
  const market = useContext(MarketContext);
  const view = useSignal<EngineView | null>(null);
  const error = useSignal("");
  const explain = useSignal(true);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    try {
      explain.value = localStorage.getItem(EXPLAIN_KEY) !== "off";
    } catch {
      // Storage unavailable: keep the default.
    }
  });

  // Reload the queues when the symbol changes or its book does.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track, cleanup }) => {
    const symbol = track(() => market.selected);
    track(() => market.books[symbol]);
    let stopped = false;
    const timer = setTimeout(
      async () => {
        try {
          const next = await getEngine(symbol, 8);
          if (!stopped && next.symbol === market.selected) {
            view.value = next;
            error.value = "";
          }
        } catch (err) {
          if (!stopped) {
            error.value =
              err instanceof ApiError
                ? err.message
                : "Can't load the queues. Retrying.";
          }
        }
      },
      view.value?.symbol === symbol ? REFRESH_MS : 0,
    );
    cleanup(() => {
      stopped = true;
      clearTimeout(timer);
    });
  });

  const symbol = market.selected;

  return (
    <div class="wrap engine">
      <div class="status-line">
        <h1>Engine</h1>
        <StreamStatus />
        <button
          type="button"
          class="btn btn-small"
          aria-pressed={explain.value ? "true" : "false"}
          onClick$={() => {
            explain.value = !explain.value;
            try {
              localStorage.setItem(EXPLAIN_KEY, explain.value ? "on" : "off");
            } catch {
              // The choice lasts until reload.
            }
          }}
        >
          Explain: {explain.value ? "on" : "off"}
        </button>
      </div>
      <SymbolCards />
      <section class="panel queue-panel" aria-labelledby="queues-title">
        <div class="panel-head">
          <h2 id="queues-title">Order queues {symbol}</h2>
          <span class="muted small">
            Best 8 levels each side · front fills first
          </span>
        </div>
        <QueueView
          view={view.value}
          error={error.value}
          explain={explain.value}
        />
      </section>
      <EngineLog symbol={symbol} explain={explain.value} />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Engine | Thande Stonks Exchange",
  meta: [
    {
      name: "description",
      content:
        "Every resting order in its price queue, and what the matching engine does with each new one.",
    },
  ],
};
