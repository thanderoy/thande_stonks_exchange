/**
 * Shared lightweight-charts helpers: colours from the page's theme tokens,
 * local-time labels, and a repaint hook for theme changes. Browser only.
 */

export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
}

/** Chart colours from the page's theme tokens, so it follows light and dark. */
export function themeOptions() {
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

const pad = (n: number) => String(n).padStart(2, "0");

/** Local "HH:MM:SS" for a Unix-seconds time. */
export function localTime(time: number): string {
  const d = new Date(time * 1000);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** Calls `repaint` when the theme changes, by toggle or system setting. Returns a stop function. */
export function onThemeChange(repaint: () => void): () => void {
  const observer = new MutationObserver(repaint);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  media.addEventListener("change", repaint);
  return () => {
    observer.disconnect();
    media.removeEventListener("change", repaint);
  };
}
