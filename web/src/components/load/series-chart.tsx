import {
  component$,
  noSerialize,
  useSignal,
  useVisibleTask$,
  type NoSerialize,
} from "@builder.io/qwik";
import type { IChartApi, ISeriesApi, UTCTimestamp } from "lightweight-charts";
import { cssVar, localTime, onThemeChange, themeOptions } from "~/lib/chart";

export interface SeriesLine {
  name: string;
  /** A theme token such as "--accent". */
  colorVar: string;
  /** Unix seconds; a null value leaves a gap. */
  points: { time: number; value: number | null }[];
}

interface SeriesChartProps {
  lines: SeriesLine[];
  /** Read out to screen readers in place of the canvas. */
  label: string;
  /** How the price axis labels values. */
  unit: "count" | "micros";
}

const formatters = {
  count: (v: number) => String(Math.round(v)),
  micros: (v: number) => `${Math.round(v)} µs`,
};

interface Handle {
  chart: IChartApi;
  series: ISeriesApi<"Line">[];
}

/** Several lines over the same time axis, redrawn whenever `lines` changes. */
export const SeriesChart = component$<SeriesChartProps>((props) => {
  const host = useSignal<HTMLDivElement>();
  const handle = useSignal<NoSerialize<Handle>>();

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
        priceFormatter: formatters[props.unit],
      },
      timeScale: {
        ...themeOptions().timeScale,
        timeVisible: true,
        secondsVisible: true,
        tickMarkFormatter: (time: unknown) => localTime(Number(time)),
      },
      crosshair: { mode: 0 },
      handleScale: false,
      handleScroll: false,
    });
    const series = props.lines.map((line) =>
      chart.addSeries(LineSeries, {
        color: cssVar(line.colorVar),
        lineWidth: 2,
        priceLineVisible: false,
        lastValueVisible: false,
      }),
    );
    handle.value = noSerialize({ chart, series });
    const stopWatching = onThemeChange(() => {
      chart.applyOptions(themeOptions());
      series.forEach((s, i) =>
        s.applyOptions({ color: cssVar(props.lines[i].colorVar) }),
      );
    });
    cleanup(() => {
      stopWatching();
      chart.remove();
      handle.value = undefined;
    });
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const h = track(() => handle.value);
    const lines = track(() => props.lines);
    if (!h) return;
    lines.forEach((line, i) => {
      h.series[i]?.setData(
        line.points.map((p) =>
          p.value == null
            ? { time: p.time as UTCTimestamp }
            : { time: p.time as UTCTimestamp, value: p.value },
        ),
      );
    });
    h.chart.timeScale().fitContent();
  });

  return (
    <div class="chart-body">
      <div
        ref={host}
        class="chart-canvas"
        role="img"
        aria-label={props.label}
      />
    </div>
  );
});
