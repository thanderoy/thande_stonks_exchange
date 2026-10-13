import {
  component$,
  Slot,
  useContext,
  useContextProvider,
  useStore,
  useVisibleTask$,
} from "@builder.io/qwik";
import { useNavigate } from "@builder.io/qwik-city";
import { ApiError, getMyAccount } from "~/api/client";
import { connectStream, streamUrl } from "~/api/stream";
import { applySnapshot, applyStreamEvent } from "~/state/apply-event";
import { loadSnapshot } from "~/state/feed";
import { GuestContext } from "~/state/guest";
import {
  MarketContext,
  initialMarketState,
  type MarketState,
} from "~/state/market";

/** Wait this long after one of the visitor's fills before reloading their account. */
const ACCOUNT_DEBOUNCE_MS = 400;
/** Retry a failed snapshot load after this long. */
const SNAPSHOT_RETRY_MS = 3000;

/**
 * Shared by every app page: one market store, one stream connection and one
 * snapshot loader, kept while the visitor moves between pages.
 */
export default component$(() => {
  const guest = useContext(GuestContext);
  const nav = useNavigate();
  const market = useStore<MarketState>(initialMarketState());
  useContextProvider(MarketContext, market);

  // No session (or it expired mid-visit): send the visitor to the join form.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(
    ({ track }) => {
      const status = track(() => guest.status);
      if (status === "anonymous") nav("/");
    },
    { strategy: "document-ready" },
  );

  // Start once the guest is known, so their own trades are recognised from
  // the first snapshot on. Re-runs (and reconnects) if the session changes.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(
    ({ track, cleanup }) => {
      const status = track(() => guest.status);
      if (status !== "joined" && status !== "error") return;
      let stopped = false;
      let retry: ReturnType<typeof setTimeout> | null = null;
      let accountTimer: ReturnType<typeof setTimeout> | null = null;

      const sync = async () => {
        if (retry) clearTimeout(retry);
        retry = null;
        try {
          const snap = await loadSnapshot();
          if (stopped) return;
          applySnapshot(market, snap, guest.clientId);
          if (!snap.account && guest.status === "joined")
            guest.status = "anonymous";
        } catch {
          if (!stopped) retry = setTimeout(sync, SNAPSHOT_RETRY_MS);
        }
      };

      const refreshAccount = () => {
        if (accountTimer) clearTimeout(accountTimer);
        accountTimer = setTimeout(async () => {
          try {
            const account = await getMyAccount();
            if (!stopped) market.account = account;
          } catch (err) {
            if (err instanceof ApiError && err.isUnauthorized) {
              guest.status = "anonymous";
            }
          }
        }, ACCOUNT_DEBOUNCE_MS);
      };

      const handle = connectStream(streamUrl(window.location), {
        onEvent: (ev) => {
          const effects = applyStreamEvent(market, ev, guest.clientId);
          if (effects.resync) sync();
          else if (effects.account) refreshAccount();
        },
        onStatus: (status) => {
          market.stream = status;
          // Events may have been missed while disconnected; start from a fresh snapshot.
          if (status === "open") sync();
        },
      });

      cleanup(() => {
        stopped = true;
        if (retry) clearTimeout(retry);
        if (accountTimer) clearTimeout(accountTimer);
        handle.close();
      });
    },
    { strategy: "document-ready" },
  );

  return <Slot />;
});
