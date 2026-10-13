import { $, component$, useContext, useSignal } from "@builder.io/qwik";
import { ApiError, cancelOrder } from "~/api/client";
import { formatPrice, formatQty } from "~/lib/format";
import { GuestContext } from "~/state/guest";
import { MarketContext } from "~/state/market";

/** The visitor's resting orders on every symbol, kept live by the stream. */
export const MyOrders = component$(() => {
  const market = useContext(MarketContext);
  const guest = useContext(GuestContext);
  const message = useSignal("");

  const cancel = $(async (orderId: string) => {
    const order = market.myOrders.find((o) => o.orderId === orderId);
    if (!order || order.status !== "resting") return;
    order.status = "cancelling";
    message.value = "";
    try {
      await cancelOrder(order.orderId, order.symbol, order.side);
      // The order_cancelled event removes it from the list.
      message.value = `Cancelled your ${order.side.toLowerCase()} of ${formatQty(order.remaining)} ${order.symbol}.`;
    } catch (err) {
      if (err instanceof ApiError && err.status === 404) {
        market.myOrders = market.myOrders.filter((o) => o.orderId !== orderId);
        message.value = "That order had already filled or been cancelled.";
        return;
      }
      if (err instanceof ApiError && err.isUnauthorized)
        guest.status = "anonymous";
      const current = market.myOrders.find((o) => o.orderId === orderId);
      if (current) current.status = "resting";
      message.value =
        err instanceof ApiError
          ? err.message
          : "Couldn't cancel the order. Try again.";
    }
  });

  return (
    <section class="panel my-orders" aria-labelledby="my-orders-title">
      <div class="panel-head">
        <h2 id="my-orders-title">Your open orders</h2>
        <span class="muted small num">{market.myOrders.length}</span>
      </div>
      {market.myOrders.length === 0 ? (
        <p class="empty">
          Nothing resting. Limit orders that don't fill straight away wait here
          until they fill or you cancel them.
        </p>
      ) : (
        <ul class="order-list">
          {market.myOrders.map((o) => {
            const label = `${o.side} ${formatQty(o.remaining)} ${o.symbol}${o.price ? ` at ${formatPrice(o.price)}` : ""}`;
            return (
              <li key={o.orderId}>
                <span class={["side-tag", o.side === "Buy" ? "buy" : "sell"]}>
                  {o.side}
                </span>
                <span class="order-desc">
                  <span class="num">{formatQty(o.remaining)}</span> {o.symbol}
                  {o.price && (
                    <>
                      {" "}
                      at <span class="num">{formatPrice(o.price)}</span>
                    </>
                  )}
                </span>
                <button
                  type="button"
                  class="btn btn-small"
                  disabled={o.status === "cancelling"}
                  aria-label={`Cancel ${label}`}
                  onClick$={() => cancel(o.orderId)}
                >
                  {o.status === "cancelling" ? "Cancelling" : "Cancel"}
                </button>
              </li>
            );
          })}
        </ul>
      )}
      <p class="panel-note" role="status" aria-live="polite">
        {message.value && <span>{message.value}</span>}
      </p>
    </section>
  );
});
