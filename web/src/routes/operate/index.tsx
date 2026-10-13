import { component$ } from "@builder.io/qwik";
import type { DocumentHead } from "@builder.io/qwik-city";
import { Placeholder } from "~/components/shell/placeholder";

export default component$(() => (
  <Placeholder
    title="Operate"
    lede="Run the exchange the way an operator would."
    items={[
      "Open, halt, resume and close the trading session for each symbol.",
      "Trip a circuit breaker and watch new orders get rejected.",
      "See the current session state and recent state changes per symbol.",
    ]}
  />
));

export const head: DocumentHead = {
  title: "Operate | Thande Stonks Exchange",
};
