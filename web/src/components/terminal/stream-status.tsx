import { component$, useContext } from "@builder.io/qwik";
import { MarketContext } from "~/state/market";

const STREAM_TEXT: Record<string, string> = {
  connecting: "Connecting to live data",
  open: "Live",
  reconnecting: "Live data lost. Reconnecting",
  closed: "Live data off",
};

/** Connection state of the live stream, plus a note after a market reset. */
export const StreamStatus = component$(() => {
  const market = useContext(MarketContext);
  return (
    <span class="stream-status">
      <span class="conn" data-state={market.stream} role="status">
        {STREAM_TEXT[market.stream]}
      </span>
      {market.resetAt > 0 && (
        <span class="reset-note" role="status">
          The market was just reset: books, positions and trades start again
          from empty.
        </span>
      )}
    </span>
  );
});
