import { $, component$, useContext, useSignal } from "@builder.io/qwik";
import { Link } from "@builder.io/qwik-city";
import {
  ApiError,
  changeSession,
  createSandbox,
  deleteSandbox,
  getTrades,
  seedSandbox,
} from "~/api/client";
import { DepthLadder } from "~/components/terminal/depth-ladder";
import {
  formatPrice,
  formatQty,
  sessionLabel,
  sessionName,
  sessionTone,
} from "~/lib/format";
import { applySnapshot } from "~/state/apply-event";
import { loadSnapshot } from "~/state/feed";
import { GuestContext } from "~/state/guest";
import { MarketContext } from "~/state/market";

type Action = "create" | "seed" | "open" | "halt" | "resume" | "delete";

/** The visitor's own symbol, with the session controls the shared ones don't give them. */
export const SandboxPanel = component$(() => {
  const market = useContext(MarketContext);
  const guest = useContext(GuestContext);
  const busy = useSignal<Action | null>(null);
  const message = useSignal<{ tone: "ok" | "error"; text: string } | null>(
    null,
  );

  const sandbox = market.sandbox;
  const symbol = sandbox?.symbol ?? "";
  const session = sandbox
    ? (market.symbols[symbol]?.session ?? sandbox.state)
    : null;
  const name = sessionName(session);

  const resync = $(async () => {
    applySnapshot(market, await loadSnapshot(), guest.clientId);
  });

  const run = $(async (action: Action) => {
    busy.value = action;
    message.value = null;
    const started = Date.now();
    try {
      switch (action) {
        case "create": {
          await createSandbox();
          await resync();
          message.value = {
            tone: "ok",
            text: "Your sandbox is ready, in pre-market. Add sample orders, then open it.",
          };
          break;
        }
        case "seed": {
          const res = await seedSandbox();
          message.value = {
            tone: "ok",
            text: res.crossed
              ? `Placed ${res.orders} sample orders. Bids are above asks, which only happens before the open. Open the session to run the auction.`
              : `Placed ${res.orders} sample orders on both sides of the book. Trade against them from the Trade page.`,
          };
          break;
        }
        case "open": {
          await changeSession(symbol, "open");
          const { trades } = await getTrades(symbol, 100);
          const auction = trades.filter(
            (t) => Date.parse(t.executed_at) >= started - 2000,
          );
          const shares = auction.reduce((n, t) => n + Number(t.quantity), 0);
          message.value = {
            tone: "ok",
            text: auction.length
              ? `Open. The auction matched ${formatQty(String(shares))} shares in ${auction.length} trades, all at ${formatPrice(auction[0].price)}: the price that matched the most shares.`
              : "Open. Nothing was crossed, so the auction had nothing to match.",
          };
          break;
        }
        case "halt": {
          await changeSession(symbol, "halt");
          message.value = {
            tone: "ok",
            text: "Halted. New orders are rejected with 409 until you resume; cancels still work.",
          };
          break;
        }
        case "resume": {
          await changeSession(symbol, "resume");
          message.value = { tone: "ok", text: "Resumed. Orders match again." };
          break;
        }
        case "delete": {
          await deleteSandbox();
          await resync();
          message.value = {
            tone: "ok",
            text: "Sandbox removed. Create another to start again from pre-market.",
          };
          break;
        }
      }
    } catch (err) {
      if (err instanceof ApiError && err.isUnauthorized)
        guest.status = "anonymous";
      message.value = {
        tone: "error",
        text:
          err instanceof ApiError
            ? err.message
            : "Something went wrong. Reload the page and try again.",
      };
    } finally {
      busy.value = null;
    }
  });

  const button = (
    action: Action,
    label: string,
    enabled: boolean,
    cls = "btn",
  ) => (
    <button
      type="button"
      class={cls}
      disabled={!enabled || busy.value !== null}
      onClick$={() => run(action)}
    >
      {busy.value === action ? "Working" : label}
    </button>
  );

  return (
    <section class="panel sandbox" aria-labelledby="sandbox-title">
      <div class="panel-head">
        <h2 id="sandbox-title">Your sandbox</h2>
        {sandbox && (
          <span class="pill" data-tone={sessionTone(session)}>
            {sessionLabel(session)}
          </span>
        )}
      </div>
      <div class="panel-body sandbox-body">
        {!sandbox ? (
          <div class="sandbox-intro">
            <p>
              A symbol of your own that only you can see and trade. Unlike the
              shared symbols, you run its session: hold it in pre-market, open
              it, halt it and resume it.
            </p>
            {button("create", "Create my sandbox", true, "btn btn-primary")}
          </div>
        ) : (
          <>
            <div class="sandbox-meta">
              <span class="sym num">{symbol}</span>
              <span class="muted small">
                Removed after 30 idle minutes or at the nightly reset.
              </span>
            </div>
            <ol class="steps">
              <li>
                <strong>Add sample orders.</strong> Before the open they cross:
                buyers bid more than sellers ask, which continuous trading would
                never allow.
              </li>
              <li>
                <strong>Open.</strong> The opening auction picks the one price
                that matches the most shares and fills the crossed orders at it.
              </li>
              <li>
                <strong>Halt.</strong> New orders get rejected with 409 until
                you resume. Cancels still work.
              </li>
            </ol>
            <div class="sandbox-actions">
              {button(
                "seed",
                "Add sample orders",
                name === "PreMarket" || name === "Open",
              )}
              {button("open", "Open", name === "PreMarket", "btn btn-primary")}
              {button("halt", "Halt", name === "Open")}
              {button("resume", "Resume", name === "Halted")}
              {button("delete", "Delete sandbox", true, "btn btn-quiet")}
            </div>
            <p class="small">
              <Link
                href="/trade/"
                onClick$={() => {
                  market.selected = symbol;
                }}
              >
                Trade {symbol} on the Trade page
              </Link>
            </p>
          </>
        )}
        <p
          class={["result", message.value?.tone]}
          role="status"
          aria-live="polite"
        >
          {message.value?.text ?? ""}
        </p>
      </div>
      {sandbox && <DepthLadder symbol={symbol} />}
    </section>
  );
});
