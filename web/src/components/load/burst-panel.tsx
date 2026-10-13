import { component$, useContext, useSignal } from "@builder.io/qwik";
import { Link } from "@builder.io/qwik-city";
import { ApiError, burstSandbox } from "~/api/client";
import { formatQty, sessionName } from "~/lib/format";
import { GuestContext } from "~/state/guest";
import { MarketContext } from "~/state/market";

const SIZES = [50, 100, 200];

/** Fires a capped burst of orders at the visitor's own open sandbox. */
export const BurstPanel = component$(() => {
  const market = useContext(MarketContext);
  const guest = useContext(GuestContext);
  const count = useSignal(200);
  const busy = useSignal(false);
  const result = useSignal<{ tone: "ok" | "error"; text: string } | null>(null);

  const sandbox = market.sandbox;
  const state = sandbox
    ? sessionName(market.symbols[sandbox.symbol]?.session ?? sandbox.state)
    : null;

  return (
    <section class="panel burst" aria-labelledby="burst-title">
      <div class="panel-head">
        <h2 id="burst-title">Burst</h2>
        <span class="muted small">Your sandbox only · once every 10 s</span>
      </div>
      <div class="panel-body burst-body">
        <p class="small muted">
          Fires random buy and sell orders from the load bot at your sandbox,
          eight at a time, through the same checks, write-ahead log and matching
          as every other order. The charts above pick it up within a second.
        </p>
        {!sandbox ? (
          <p class="small">
            You need a sandbox first.{" "}
            <Link href="/operate/">Create one on the Operate page</Link>, then
            open it.
          </p>
        ) : state !== "Open" ? (
          <p class="small">
            Your sandbox {sandbox.symbol} isn't open.{" "}
            <Link href="/operate/">Open it on the Operate page</Link>.
          </p>
        ) : (
          <div class="burst-row">
            <fieldset class="segmented">
              <legend class="visually-hidden">Orders per burst</legend>
              {SIZES.map((n) => (
                <label key={n}>
                  <input
                    type="radio"
                    name="burst-size"
                    value={n}
                    checked={count.value === n}
                    onChange$={() => (count.value = n)}
                  />
                  {n}
                </label>
              ))}
            </fieldset>
            <button
              type="button"
              class="btn btn-primary"
              disabled={busy.value}
              onClick$={async () => {
                busy.value = true;
                result.value = null;
                try {
                  const r = await burstSandbox(count.value);
                  result.value = {
                    tone: "ok",
                    text:
                      `${formatQty(String(r.accepted))} of ${formatQty(String(r.submitted))} orders accepted in ${r.elapsed_ms} ms ` +
                      `(${formatQty(String(r.orders_per_sec))} orders/s), ${formatQty(String(r.trades))} trades. ` +
                      `Latency p50 ${r.p50_us} µs, p99 ${r.p99_us} µs. ` +
                      `${formatQty(String(r.cancelled))} leftover orders were cancelled.`,
                  };
                } catch (err) {
                  if (err instanceof ApiError && err.isUnauthorized)
                    guest.status = "anonymous";
                  result.value = {
                    tone: "error",
                    text:
                      err instanceof ApiError
                        ? err.message
                        : "Something went wrong. Reload the page and try again.",
                  };
                } finally {
                  busy.value = false;
                }
              }}
            >
              {busy.value
                ? "Firing"
                : `Fire ${count.value} orders at ${sandbox.symbol}`}
            </button>
          </div>
        )}
        <p
          class={["result", result.value?.tone]}
          role="status"
          aria-live="polite"
        >
          {result.value && <span>{result.value.text}</span>}
        </p>
      </div>
    </section>
  );
});
