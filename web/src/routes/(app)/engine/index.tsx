import { component$ } from "@builder.io/qwik";
import type { DocumentHead } from "@builder.io/qwik-city";
import { Placeholder } from "~/components/shell/placeholder";

export default component$(() => (
  <Placeholder
    title="Engine"
    lede="Look inside the matching engine while it runs."
    items={[
      "Step through how an incoming order walks the book, level by level.",
      "See price-time priority decide which resting order fills first.",
      "Follow each order from the write-ahead log to the trade ledger.",
    ]}
  />
));

export const head: DocumentHead = {
  title: "Engine | Thande Stonks Exchange",
};
