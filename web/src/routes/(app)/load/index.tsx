import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import type { DocumentHead } from "@builder.io/qwik-city";
import { getStats } from "~/api/client";
import type { Stats } from "~/api/types";
import { BurstPanel } from "~/components/load/burst-panel";
import { SeriesChart } from "~/components/load/series-chart";
import { StreamStatus } from "~/components/terminal/stream-status";
import { formatQty } from "~/lib/format";

const POLL_MS = 1000;

const micros = (v: number | null | undefined) =>
  v == null ? "–" : v >= 1000 ? `${(v / 1000).toFixed(2)} ms` : `${v} µs`;

export default component$(() => {
  const stats = useSignal<Stats | null>(null);
  const error = useSignal("");

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    let stopped = false;
    const load = async () => {
      if (document.hidden) return;
      try {
        const next = await getStats();
        if (!stopped) {
          stats.value = next;
          error.value = "";
        }
      } catch {
        if (!stopped) error.value = "Can't load the stats. Retrying.";
      }
    };
    load();
    const timer = setInterval(load, POLL_MS);
    cleanup(() => {
      stopped = true;
      clearInterval(timer);
    });
  });

  const s = stats.value;
  // The newest second is still filling up, so rates use the one before it.
  const last = s?.seconds[s.seconds.length - 2];
  const seconds = s?.seconds ?? [];
  const throughput = [
    {
      name: "Orders",
      colorVar: "--accent",
      points: seconds.map((x) => ({ time: x.t, value: x.orders })),
    },
    {
      name: "Trades",
      colorVar: "--bid",
      points: seconds.map((x) => ({ time: x.t, value: x.trades })),
    },
  ];
  const latency = [
    {
      name: "p50",
      colorVar: "--accent",
      points: seconds.map((x) => ({ time: x.t, value: x.p50_us })),
    },
    {
      name: "p99",
      colorVar: "--ask",
      points: seconds.map((x) => ({ time: x.t, value: x.p99_us })),
    },
  ];

  const tiles: [string, string][] = [
    ["Orders/s", last ? formatQty(String(last.orders)) : "–"],
    ["Trades/s", last ? formatQty(String(last.trades)) : "–"],
    ["Latency p50 (60 s)", micros(s?.p50_us)],
    ["Latency p99 (60 s)", micros(s?.p99_us)],
    ["Orders since start", s ? formatQty(String(s.orders)) : "–"],
    ["Rejected", s ? formatQty(String(s.rejected)) : "–"],
    ["Trades since start", s ? formatQty(String(s.trades)) : "–"],
  ];

  return (
    <div class="wrap load">
      <div class="status-line">
        <h1>Load</h1>
        <StreamStatus />
      </div>
      <p class="operate-lede muted">
        Measured inside the server across every symbol: how many orders arrive
        each second and how long each takes, from waiting for its symbol's lock
        to the engine's reply. Bots alone send a few a second; a burst shows
        what the engine does with more.
      </p>
      {error.value && (
        <p class="notice error" role="status">
          {error.value}
        </p>
      )}
      <div class="tiles" role="list" aria-label="Current figures">
        {tiles.map(([label, value]) => (
          <div class="tile" role="listitem" key={label}>
            <span class="v num">{value}</span>
            <span class="l">{label}</span>
          </div>
        ))}
      </div>
      <div class="load-charts">
        <section class="panel" aria-labelledby="thru-title">
          <div class="panel-head">
            <h2 id="thru-title">Throughput per second</h2>
            <span class="legend">
              <span>
                <i style={{ background: "var(--accent)" }} />
                Orders
              </span>
              <span>
                <i style={{ background: "var(--bid)" }} />
                Trades
              </span>
            </span>
          </div>
          <SeriesChart
            lines={throughput}
            label={`Orders and trades per second over the last minute. Last full second: ${last?.orders ?? 0} orders, ${last?.trades ?? 0} trades.`}
            unit="count"
          />
        </section>
        <section class="panel" aria-labelledby="lat-title">
          <div class="panel-head">
            <h2 id="lat-title">Submit latency</h2>
            <span class="legend">
              <span>
                <i style={{ background: "var(--accent)" }} />
                p50
              </span>
              <span>
                <i style={{ background: "var(--ask)" }} />
                p99
              </span>
            </span>
          </div>
          <SeriesChart
            lines={latency}
            label={`Submit latency over the last minute, in microseconds. Over the minute: p50 ${micros(s?.p50_us)}, p99 ${micros(s?.p99_us)}.`}
            unit="micros"
          />
        </section>
      </div>
      <BurstPanel />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Load | Thande Stonks Exchange",
  meta: [
    {
      name: "description",
      content:
        "Orders, trades and latency per second, and load bursts against your sandbox.",
    },
  ],
};
