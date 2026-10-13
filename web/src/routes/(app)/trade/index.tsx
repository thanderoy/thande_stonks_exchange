import { component$, useContext } from "@builder.io/qwik";
import type { DocumentHead } from "@builder.io/qwik-city";
import { DepthLadder } from "~/components/terminal/depth-ladder";
import { MyOrders } from "~/components/terminal/my-orders";
import { OrderTicket } from "~/components/terminal/order-ticket";
import { Positions } from "~/components/terminal/positions";
import { PriceChart } from "~/components/terminal/price-chart";
import { StreamStatus } from "~/components/terminal/stream-status";
import { SymbolCards } from "~/components/terminal/symbol-cards";
import { TradeTape } from "~/components/terminal/trade-tape";
import { formatPrice } from "~/lib/format";
import { MarketContext } from "~/state/market";

export default component$(() => {
  const market = useContext(MarketContext);
  const s = market.symbols[market.selected];

  return (
    <div class="wrap terminal">
      <div class="status-line">
        <h1 class="visually-hidden">Trade</h1>
        <StreamStatus />
        <span class="muted">
          {s.collarLow && s.collarHigh
            ? `Limit prices from ${formatPrice(s.collarLow)} to ${formatPrice(s.collarHigh)}`
            : "Limit prices from 80.00 to 120.00"}
        </span>
      </div>
      <SymbolCards />
      <PriceChart />
      <DepthLadder />
      <TradeTape />
      <OrderTicket />
      <MyOrders />
      <Positions />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Trade | Thande Stonks Exchange",
  meta: [
    {
      name: "description",
      content:
        "Live order book, price chart, trade tape, order entry and your positions.",
    },
  ],
};
