import {
  $,
  component$,
  useContext,
  useSignal,
  useTask$,
} from "@builder.io/qwik";
import { ApiError, submitOrder } from "~/api/client";
import type { Side, SubmitOrderRequest, TimeInForce } from "~/api/types";
import {
  PRICE_MAX,
  PRICE_MIN,
  summarizeOrderResult,
  tifLabel,
  validateOrderForm,
  type OrderFormErrors,
} from "~/lib/format";
import { addPlacedOrder } from "~/state/apply-event";
import { GuestContext } from "~/state/guest";
import { MarketContext } from "~/state/market";

const TIFS: TimeInForce[] = [
  "GoodTillCanceled",
  "Day",
  "ImmediateOrCancel",
  "FillOrKill",
];

export const OrderTicket = component$(() => {
  const market = useContext(MarketContext);
  const guest = useContext(GuestContext);

  const side = useSignal<Side>("Buy");
  const kind = useSignal<"Limit" | "Market">("Limit");
  const price = useSignal("100.00");
  const quantity = useSignal("100");
  const tif = useSignal<TimeInForce>("GoodTillCanceled");
  const errors = useSignal<OrderFormErrors>({});
  const result = useSignal<{ tone: "ok" | "error"; text: string } | null>(null);
  const busy = useSignal(false);

  // A click on a ladder price copies it here.
  useTask$(({ track }) => {
    track(() => market.pickedAt);
    if (market.pickedPrice) {
      price.value = market.pickedPrice;
      kind.value = "Limit";
    }
  });

  const handleError = $((err: unknown): string => {
    if (err instanceof ApiError) {
      if (err.isUnauthorized) guest.status = "anonymous";
      return err.message;
    }
    return "Something went wrong. Reload the page and try again.";
  });

  const submit = $(async () => {
    const found = validateOrderForm({
      kind: kind.value,
      price: price.value,
      quantity: quantity.value,
    });
    errors.value = found;
    if (found.price || found.quantity) {
      result.value = {
        tone: "error",
        text: "Fix the highlighted fields and submit again.",
      };
      return;
    }

    const symbol = market.selected;
    const order: SubmitOrderRequest = {
      symbol,
      side: side.value,
      order_type:
        kind.value === "Limit"
          ? { type: "Limit", price: price.value.trim() }
          : { type: "Market" },
      quantity: quantity.value.trim(),
      time_in_force: tif.value,
    };

    busy.value = true;
    result.value = null;
    try {
      const res = await submitOrder(order);
      result.value = {
        tone: "ok",
        text: summarizeOrderResult(
          res.trades,
          order.quantity,
          kind.value,
          tif.value,
        ),
      };
      if (res.status === "resting") {
        addPlacedOrder(market, {
          orderId: res.order_id,
          symbol,
          side: order.side,
          price:
            order.order_type.type === "Limit" ? order.order_type.price : null,
          remaining: res.remaining,
          status: "resting",
          confirmed: false,
        });
      }
    } catch (err) {
      result.value = { tone: "error", text: await handleError(err) };
    } finally {
      busy.value = false;
    }
  });

  return (
    <section class="panel ticket" aria-labelledby="ticket-title">
      <div class="panel-head">
        <h2 id="ticket-title">New order {market.selected}</h2>
      </div>
      <div class="panel-body">
        <form preventdefault:submit onSubmit$={submit} noValidate>
          <fieldset class="segmented side">
            <legend class="visually-hidden">Side</legend>
            <label class="buy">
              <input
                type="radio"
                name="side"
                value="Buy"
                checked={side.value === "Buy"}
                onChange$={() => (side.value = "Buy")}
              />
              Buy
            </label>
            <label class="sell">
              <input
                type="radio"
                name="side"
                value="Sell"
                checked={side.value === "Sell"}
                onChange$={() => (side.value = "Sell")}
              />
              Sell
            </label>
          </fieldset>

          <fieldset class="segmented">
            <legend class="visually-hidden">Order type</legend>
            <label>
              <input
                type="radio"
                name="kind"
                value="Limit"
                checked={kind.value === "Limit"}
                onChange$={() => (kind.value = "Limit")}
              />
              Limit
            </label>
            <label>
              <input
                type="radio"
                name="kind"
                value="Market"
                checked={kind.value === "Market"}
                onChange$={() => (kind.value = "Market")}
              />
              Market
            </label>
          </fieldset>

          <div class="row">
            <div class="field">
              <label for="order-price">Limit price</label>
              <input
                id="order-price"
                class="input num"
                inputMode="decimal"
                autoComplete="off"
                disabled={kind.value === "Market"}
                bind:value={price}
                aria-invalid={errors.value.price ? "true" : undefined}
                aria-describedby="order-price-hint order-price-error"
              />
              <span id="order-price-hint" class="visually-hidden">
                Between {PRICE_MIN} and {PRICE_MAX}.
              </span>
              <span id="order-price-error" class="field-error">
                {kind.value === "Limit" ? errors.value.price : ""}
              </span>
            </div>
            <div class="field">
              <label for="order-qty">Quantity</label>
              <input
                id="order-qty"
                class="input num"
                inputMode="decimal"
                autoComplete="off"
                bind:value={quantity}
                aria-invalid={errors.value.quantity ? "true" : undefined}
                aria-describedby="order-qty-error"
              />
              <span id="order-qty-error" class="field-error">
                {errors.value.quantity}
              </span>
            </div>
          </div>

          <div class="field">
            <label for="order-tif">Time in force</label>
            <select
              id="order-tif"
              class="select"
              value={tif.value}
              onChange$={(_, el) => (tif.value = el.value as TimeInForce)}
            >
              {TIFS.map((t) => (
                <option key={t} value={t} selected={tif.value === t}>
                  {tifLabel(t)}
                </option>
              ))}
            </select>
          </div>

          <button
            type="submit"
            class={["btn", side.value === "Buy" ? "btn-buy" : "btn-sell"]}
            disabled={busy.value}
          >
            {busy.value
              ? "Sending"
              : `${side.value} ${market.selected}${kind.value === "Market" ? " at market" : ""}`}
          </button>

          <p
            class={["result", result.value?.tone]}
            aria-live="polite"
            role="status"
          >
            {result.value && <span>{result.value.text}</span>}
          </p>
        </form>
      </div>
    </section>
  );
});
