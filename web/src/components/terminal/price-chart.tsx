import {
  component$,
  noSerialize,
  useContext,
  useSignal,
  useVisibleTask$,
  type NoSerialize,
} from "@builder.io/qwik";
import type { IChartApi, ISeriesApi, UTCTimestamp } from "lightweight-charts";
import { formatPrice } from "~/lib/format";
import { MarketContext, type PricePoint } from "~/state/market";

interface ChartHandle {
  chart: IChartApi;
  line: ISeriesApi<"Line">;
  symbol: string;
  points: number;
}

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
}

/** Chart colours from the page's theme tokens, so it follows light and dark. */
function themeOptions() {
  return {
    layout: {
      background: { color: cssVar("--surface") },
      textColor: cssVar("--muted"),
      fontFamily: cssVar("--font-num") || "monospace",
    },
    grid: {
      vertLines: { color: cssVar("--line") },
      horzLines: { color: cssVar("--line") },
    },
    rightPriceScale: { borderColor: cssVar("--line") },
    timeScale: { borderColor: cssVar("--line") },
  };
}

const toData = (points: PricePoint[]) =>
  points.map((p) => ({ time: p.time as UTCTimestamp, value: p.price }));

const pad = (n: number) => String(n).padStart(2, "0");

/** Local "HH:MM:SS" for a Unix-seconds time. */
function localTime(time: number): string {
  const d = new Date(time * 1000);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export const PriceChart = component$(() => {
  const market = useContext(MarketContext);
  const host = useSignal<HTMLDivElement>();
  const handle = useSignal<NoSerialize<ChartHandle>>();

  // Create the chart once, in the browser only.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const el = host.value;
    if (!el) return;
    const { createChart, LineSeries } = await import("lightweight-charts");
    const chart = createChart(el, {
      ...themeOptions(),
      autoSize: true,
      localization: {
        timeFormatter: (time: unknown) => localTime(Number(time)),
        priceFormatter: (p: number) => p.toFixed(2),
      },
      timeScale: {
        ...themeOptions().timeScale,
        timeVisible: true,
        secondsVisible: true,
        tickMarkFormatter: (time: unknown) =>
          localTime(Number(time)).slice(0, 5),
      },
      crosshair: { mode: 0 },
      handleScale: false,
      handleScroll: false,
    });
    const line = chart.addSeries(LineSeries, {
      color: cssVar("--accent"),
      lineWidth: 2,
      priceLineColor: cssVar("--accent"),
      lastValueVisible: true,
    });
    handle.value = noSerialize({ chart, line, symbol: "", points: 0 });

    // Repaint when the theme changes, by toggle or by the system setting.
    const repaint = () => {
      chart.applyOptions(themeOptions());
      line.applyOptions({
        color: cssVar("--accent"),
        priceLineColor: cssVar("--accent"),
      });
    };
    const observer = new MutationObserver(repaint);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", repaint);

    cleanup(() => {
      observer.disconnect();
      media.removeEventListener("change", repaint);
      chart.remove();
      handle.value = undefined;
    });
  });

  // Feed it: a full redraw on a symbol switch or a reset, otherwise just the newest point.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const h = track(() => handle.value);
    const symbol = track(() => market.selected);
    const points = track(() => market.series[symbol]) ?? [];
    track(() => points.length);
    track(() => points[points.length - 1]?.price);
    if (!h) return;
    if (
      h.symbol !== symbol ||
      points.length < h.points ||
      points.length - h.points > 1
    ) {
      h.line.setData(toData(points));
      h.chart.timeScale().fitContent();
    } else if (points.length > 0) {
      const last = points[points.length - 1];
      h.line.update({ time: last.time as UTCTimestamp, value: last.price });
      if (points.length !== h.points) h.chart.timeScale().fitContent();
    }
    h.symbol = symbol;
    h.points = points.length;
  });

  const summary = market.symbols[market.selected];
  const points = market.series[market.selected] ?? [];

  return (
    <section class="panel chart" aria-labelledby="chart-title">
      <div class="panel-head">
        <h2 id="chart-title">Price {market.selected}</h2>
        <span class="muted small">
          {summary.high && summary.low ? (
            <>
              High <span class="num">{formatPrice(summary.high)}</span> · Low{" "}
              <span class="num">{formatPrice(summary.low)}</span>
            </>
          ) : (
            "Trades since the last reset"
          )}
        </span>
      </div>
      <div class="chart-body">
        <div
          ref={host}
          class="chart-canvas"
          role="img"
          aria-label={
            summary.last
              ? `Price chart for ${market.selected}, last trade ${formatPrice(summary.last)}.`
              : `Price chart for ${market.selected}. No trades yet.`
          }
        />
        {points.length === 0 && (
          <p class="chart-empty muted">
            No trades in {market.selected} yet. The line starts with the first
            trade.
          </p>
        )}
      </div>
    </section>
  );
});
