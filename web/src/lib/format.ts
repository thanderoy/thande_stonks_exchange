/**
 * Display formatting and input validation. Pure functions, no DOM.
 *
 * Prices and quantities arrive as decimal strings. Formatting works on the
 * string so nothing is rounded through a float; `Number()` is only used for
 * comparisons that affect presentation (tick direction, depth bar widths).
 */
import type { SessionState } from "~/api/types";

const DECIMAL_RE = /^\d+(\.\d+)?$/;

/** Split "1234.5" into ["1234", "5"], tolerating a leading "+" or whitespace. */
function splitDecimal(value: string): [string, string] | null {
  const v = value.trim().replace(/^\+/, "");
  if (!DECIMAL_RE.test(v)) return null;
  const [int, frac = ""] = v.split(".");
  return [int.replace(/^0+(?=\d)/, ""), frac];
}

function groupThousands(int: string): string {
  return int.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

/** "100" -> "100.00", "100.5" -> "100.50", "100.12500000" -> "100.125". */
export function formatPrice(value: string | null | undefined): string {
  if (value == null) return "";
  const parts = splitDecimal(value);
  if (!parts) return value;
  const [int, rawFrac] = parts;
  let frac = rawFrac.replace(/0+$/, "");
  if (frac.length < 2) frac = frac.padEnd(2, "0");
  return `${groupThousands(int)}.${frac}`;
}

/** "1500.00000000" -> "1,500", "2.5" -> "2.5". */
export function formatQty(value: string | null | undefined): string {
  if (value == null) return "";
  const parts = splitDecimal(value);
  if (!parts) return value;
  const [int, rawFrac] = parts;
  const frac = rawFrac.replace(/0+$/, "");
  return frac ? `${groupThousands(int)}.${frac}` : groupThousands(int);
}

/** 24-hour local time, "14:03:09". Empty string for an unparseable input. */
export function formatTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export type Tick = "up" | "down" | "flat";

/** Direction of `next` relative to `prev`; "flat" when either is missing. */
export function tickDirection(prev: string | null | undefined, next: string | null | undefined): Tick {
  if (prev == null || next == null) return "flat";
  const a = Number(prev);
  const b = Number(next);
  if (!Number.isFinite(a) || !Number.isFinite(b) || a === b) return "flat";
  return b > a ? "up" : "down";
}

// ── Session state ────────────────────────────────────────────────────────────

export type SessionTone = "open" | "auction" | "halted" | "closed" | "unknown";

/** Normalise REST (`{"Halted": {...}}`) and stream (`"Halted"`) shapes to a name. */
export function sessionName(state: SessionState | string | null | undefined): string | null {
  if (state == null) return null;
  if (typeof state === "string") return state.startsWith("Halted") ? "Halted" : state;
  if (typeof state === "object" && "Halted" in state) return "Halted";
  return null;
}

export function sessionLabel(state: SessionState | string | null | undefined): string {
  switch (sessionName(state)) {
    case "PreMarket":
      return "Pre-market";
    case "Open":
      return "Open";
    case "PostMarket":
      return "Post-market";
    case "Closed":
      return "Closed";
    case "Halted":
      return "Halted";
    case null:
      return "Unknown";
    default:
      return String(sessionName(state));
  }
}

export function sessionTone(state: SessionState | string | null | undefined): SessionTone {
  switch (sessionName(state)) {
    case "Open":
      return "open";
    case "PreMarket":
    case "PostMarket":
      return "auction";
    case "Halted":
      return "halted";
    case "Closed":
      return "closed";
    default:
      return "unknown";
  }
}

export const COMPANY_NAMES: Record<string, string> = {
  AAPL: "Apple",
  MSFT: "Microsoft",
  GOOGL: "Alphabet",
};

const TIF_LABELS: Record<string, string> = {
  GoodTillCanceled: "Good till cancelled",
  ImmediateOrCancel: "Immediate or cancel",
  FillOrKill: "Fill or kill",
  Day: "Day",
};

export function tifLabel(tif: string): string {
  return TIF_LABELS[tif] ?? tif;
}

/**
 * One-line description of what happened to a submitted order, for the
 * order ticket's live region. Totals are for display only.
 */
export function summarizeOrderResult(
  trades: { price: string; quantity: string }[],
  requested: string,
  kind: "Limit" | "Market",
  tif: string,
): string {
  if (trades.length === 0) {
    if (kind === "Market") return "Order accepted. Nothing matched, so it did not fill.";
    if (tif === "ImmediateOrCancel" || tif === "FillOrKill") {
      return "Order accepted. Nothing matched, so it was cancelled.";
    }
    return "Order accepted. Nothing matched yet, so it is resting on the book.";
  }
  let filled = 0;
  let notional = 0;
  for (const t of trades) {
    const q = Number(t.quantity);
    filled += q;
    notional += q * Number(t.price);
  }
  const avg = filled > 0 ? (notional / filled).toFixed(2) : "0.00";
  const tradesText = trades.length === 1 ? "1 trade" : `${trades.length} trades`;
  const req = Number(requested);
  const full = Number.isFinite(req) && filled >= req;
  const filledText = formatQty(String(Number(filled.toFixed(8))));
  if (full) return `Filled ${filledText} in ${tradesText} at an average of ${formatPrice(avg)}.`;
  const rest =
    kind === "Limit" && (tif === "GoodTillCanceled" || tif === "Day")
      ? "The rest is resting on the book."
      : "The rest was cancelled.";
  return `Filled ${filledText} of ${formatQty(requested)} in ${tradesText} at an average of ${formatPrice(avg)}. ${rest}`;
}

// ── Validation ───────────────────────────────────────────────────────────────

export const NAME_MIN = 2;
export const NAME_MAX = 24;
const NAME_RE = /^[\p{L}\p{N} _-]+$/u;

/** Error message for a display name, or null when it is acceptable. */
export function validateDisplayName(raw: string): string | null {
  const name = raw.trim();
  if (name.length < NAME_MIN || name.length > NAME_MAX) {
    return `Use ${NAME_MIN} to ${NAME_MAX} characters.`;
  }
  if (!NAME_RE.test(name)) {
    return "Use only letters, digits, spaces, underscores and hyphens.";
  }
  return null;
}

export const PRICE_MIN = 80;
export const PRICE_MAX = 120;
const INPUT_DECIMAL_RE = /^\d+(\.\d{1,8})?$/;

export interface OrderFormValues {
  kind: "Limit" | "Market";
  price: string;
  quantity: string;
}

export interface OrderFormErrors {
  price?: string;
  quantity?: string;
}

/** Client-side checks that mirror the server's, so most mistakes never leave the page. */
export function validateOrderForm(v: OrderFormValues): OrderFormErrors {
  const errors: OrderFormErrors = {};
  if (v.kind === "Limit") {
    const p = v.price.trim();
    if (!p) {
      errors.price = "Enter a limit price.";
    } else if (!INPUT_DECIMAL_RE.test(p)) {
      errors.price = "Enter the price as a number, like 100.25.";
    } else if (Number(p) < PRICE_MIN || Number(p) > PRICE_MAX) {
      errors.price = `Enter a price between ${PRICE_MIN}.00 and ${PRICE_MAX}.00.`;
    }
  }
  const q = v.quantity.trim();
  if (!q) {
    errors.quantity = "Enter a quantity.";
  } else if (!INPUT_DECIMAL_RE.test(q)) {
    errors.quantity = "Enter the quantity as a number, like 100.";
  } else if (Number(q) <= 0) {
    errors.quantity = "Enter a quantity greater than 0.";
  }
  return errors;
}
