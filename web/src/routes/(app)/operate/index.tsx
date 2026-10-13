import { component$, useContext, useVisibleTask$ } from "@builder.io/qwik";
import type { DocumentHead } from "@builder.io/qwik-city";
import { getLeaderboard } from "~/api/client";
import { Leaderboard } from "~/components/operate/leaderboard";
import { SessionBoard } from "~/components/operate/session-board";
import { Positions } from "~/components/terminal/positions";
import { StreamStatus } from "~/components/terminal/stream-status";
import { MarketContext } from "~/state/market";

/** How often the leaderboard is reloaded while this page is open. */
const LEADERBOARD_POLL_MS = 5000;

export default component$(() => {
  const market = useContext(MarketContext);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(
    ({ cleanup }) => {
      let stopped = false;
      const load = async () => {
        if (document.hidden) return;
        try {
          const board = await getLeaderboard(10);
          if (!stopped) market.leaderboard = board.entries;
        } catch {
          // Keep the last board; the next poll retries.
        }
      };
      load();
      const timer = setInterval(load, LEADERBOARD_POLL_MS);
      cleanup(() => {
        stopped = true;
        clearInterval(timer);
      });
    },
    { strategy: "document-ready" },
  );

  return (
    <div class="wrap operate">
      <div class="status-line">
        <h1>Operate</h1>
        <StreamStatus />
      </div>
      <p class="operate-lede muted">
        The shared market as an operator sees it. Bots keep it trading, open it
        after each nightly reset and leave it alone while it is halted. Session
        controls here are read only: opening, halting and resuming the shared
        symbols is for the operator. Controls for a sandbox symbol of your own
        come in a later release.
      </p>
      <SessionBoard />
      <Leaderboard />
      <Positions />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Operate | Thande Stonks Exchange",
  meta: [
    {
      name: "description",
      content: "Session state, market statistics and the leaderboard.",
    },
  ],
};
