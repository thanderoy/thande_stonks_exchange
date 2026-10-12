import {
  component$,
  useContext,
  useContextProvider,
  useStore,
  useVisibleTask$,
} from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { ApiError, getBook, getSession } from "~/api/client";
import { connectStream, streamUrl } from "~/api/stream";
import { SYMBOLS } from "~/api/types";
import { DepthLadder } from "~/components/terminal/depth-ladder";
import { OrderTicket } from "~/components/terminal/order-ticket";
import { SymbolCards } from "~/components/terminal/symbol-cards";
import { TradeTape } from "~/components/terminal/trade-tape";
import { applyStreamEvent } from "~/state/apply-event";
import { GuestContext } from "~/state/guest";
import {
  MarketContext,
  initialMarketState,
  type MarketState,
} from "~/state/market";

const BOOK_POLL_MS = 1000;

const STREAM_TEXT: Record<string, string> = {
  connecting: "Connecting to live data",
  open: "Live",
  reconnecting: "Live data lost. Reconnecting",
  closed: "Live data off",
};

export default component$(() => {
  const guest = useContext(GuestContext);
  const nav = useNavigate();
  const market = useStore<MarketState>(initialMarketState());
  useContextProvider(MarketContext, market);

  // No session (or it expired mid-visit): send the visitor to the join form.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const status = track(() => guest.status);
    if (status === "anonymous") nav("/");
  });

  // Live stream: trades, session changes, cancels.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const refreshSessions = () =>
      Promise.all(
        SYMBOLS.map(async (sym) => {
          try {
            const res = await getSession(sym);
            market.symbols[sym].session = res.state;
          } catch (err) {
            if (err instanceof ApiError && err.isUnauthorized) {
              guest.status = "anonymous";
            }
          }
        }),
      );

    const handle = connectStream(streamUrl(window.location), {
      onEvent: (ev) => applyStreamEvent(market, ev, guest.clientId),
      onStatus: (status) => {
        market.stream = status;
        // Events may have been missed while disconnected; resync sessions.
        if (status === "open") refreshSessions();
      },
    });
    refreshSessions();
    cleanup(() => handle.close());
  });

  // Depth ladder: poll the selected symbol's book. Push can replace this later.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track, cleanup }) => {
    const symbol = track(() => market.selected);
    let inFlight = false;
    let stopped = false;

    const poll = async () => {
      if (inFlight || stopped || document.hidden) return;
      inFlight = true;
      try {
        const book = await getBook(symbol);
        if (stopped) return;
        market.book = {
          symbol,
          asks: book.asks,
          bids: book.bids,
          loaded: true,
          error: "",
        };
      } catch (err) {
        if (stopped) return;
        if (err instanceof ApiError && err.isUnauthorized) {
          guest.status = "anonymous";
          return;
        }
        market.book.error =
          err instanceof ApiError
            ? err.message
            : "Can't load the order book. Retrying.";
      } finally {
        inFlight = false;
      }
    };

    poll();
    const timer = setInterval(poll, BOOK_POLL_MS);
    cleanup(() => {
      stopped = true;
      clearInterval(timer);
    });
  });

  return (
    <div class="wrap terminal">
      <div class="status-line">
        <h1 class="visually-hidden">Trade</h1>
        <span class="conn" data-state={market.stream} role="status">
          {STREAM_TEXT[market.stream]}
        </span>
        <span class="muted">Limit prices from 80.00 to 120.00</span>
      </div>
      <SymbolCards />
      <DepthLadder />
      <TradeTape />
      <OrderTicket />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Trade | Thande Stonks Exchange",
  meta: [
    {
      name: "description",
      content: "Live order book, trade tape and order entry.",
    },
  ],
};
