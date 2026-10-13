import { component$ } from "@builder.io/qwik";
import type { DocumentHead } from "@builder.io/qwik-city";
import { Placeholder } from "~/components/shell/placeholder";

export default component$(() => (
  <Placeholder
    title="Load"
    lede="Put the engine under pressure and measure how it holds up."
    items={[
      "Start a burst of bot orders at a rate you choose.",
      "Watch orders per second, trades per second and latency as it runs.",
      "Compare a run against earlier ones.",
    ]}
  />
));

export const head: DocumentHead = {
  title: "Load | Thande Stonks Exchange",
};
