/**
 * WebSocket client for `/v1/stream` with automatic reconnect.
 *
 * Reconnects use exponential backoff with jitter, capped at `maxDelayMs`, and
 * reset once a connection opens. Messages may carry one JSON event or several
 * newline-delimited events; anything that does not parse is dropped.
 */
import { API_BASE } from "./client";
import type { StreamEvent } from "./types";

export type StreamStatus = "connecting" | "open" | "reconnecting" | "closed";

export interface StreamOptions {
  onEvent: (event: StreamEvent) => void;
  onStatus?: (status: StreamStatus, retryInMs?: number) => void;
  baseDelayMs?: number;
  maxDelayMs?: number;
  /** Injected for tests. */
  createSocket?: (url: string) => WebSocket;
  random?: () => number;
}

/**
 * Delay before reconnect attempt `attempt` (0-based): base * 2^attempt,
 * capped, with up to 25% subtracted at random so many tabs don't reconnect in
 * lockstep after a server restart.
 */
export function backoffDelay(
  attempt: number,
  baseDelayMs = 500,
  maxDelayMs = 15_000,
  random: () => number = Math.random,
): number {
  const exp = Math.min(maxDelayMs, baseDelayMs * 2 ** Math.max(0, attempt));
  return Math.round(exp - exp * 0.25 * random());
}

/** `ws(s)://<host>/v1/stream` for the page's own origin. */
export function streamUrl(loc: Pick<Location, "protocol" | "host">): string {
  const scheme = loc.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${loc.host}${API_BASE}/stream`;
}

const KNOWN_EVENTS = new Set([
  "trade",
  "order_accepted",
  "order_cancelled",
  "session_state",
  "book_update",
  "market_reset",
]);

/** Parse one WebSocket message into zero or more events. */
export function parseStreamMessage(data: string): StreamEvent[] {
  const events: StreamEvent[] = [];
  for (const line of data.split("\n")) {
    const text = line.trim();
    if (!text) continue;
    try {
      const value: unknown = JSON.parse(text);
      if (
        value &&
        typeof value === "object" &&
        "event" in value &&
        KNOWN_EVENTS.has((value as { event: string }).event)
      ) {
        events.push(value as StreamEvent);
      }
    } catch {
      // Ignore malformed frames rather than tearing down the connection.
    }
  }
  return events;
}

export interface StreamHandle {
  close: () => void;
}

export function connectStream(url: string, opts: StreamOptions): StreamHandle {
  const base = opts.baseDelayMs ?? 500;
  const max = opts.maxDelayMs ?? 15_000;
  const make = opts.createSocket ?? ((u: string) => new WebSocket(u));
  const random = opts.random ?? Math.random;

  let socket: WebSocket | null = null;
  let attempt = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let stopped = false;

  const open = () => {
    if (stopped) return;
    opts.onStatus?.(attempt === 0 ? "connecting" : "reconnecting");
    try {
      socket = make(url);
    } catch {
      scheduleReconnect();
      return;
    }
    socket.onopen = () => {
      attempt = 0;
      opts.onStatus?.("open");
    };
    socket.onmessage = (msg: MessageEvent) => {
      if (typeof msg.data !== "string") return;
      for (const ev of parseStreamMessage(msg.data)) opts.onEvent(ev);
    };
    socket.onclose = () => {
      socket = null;
      scheduleReconnect();
    };
    // `error` is always followed by `close`; reconnect is handled there.
    socket.onerror = () => {};
  };

  const scheduleReconnect = () => {
    if (stopped) return;
    const delay = backoffDelay(attempt, base, max, random);
    attempt += 1;
    opts.onStatus?.("reconnecting", delay);
    timer = setTimeout(open, delay);
  };

  open();

  return {
    close() {
      stopped = true;
      if (timer) clearTimeout(timer);
      timer = null;
      if (socket) {
        socket.onclose = null;
        socket.close();
        socket = null;
      }
      opts.onStatus?.("closed");
    },
  };
}
