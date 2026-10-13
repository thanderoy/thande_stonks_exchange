/**
 * Typed fetch helpers for the exchange `/v1` API.
 *
 * Every call goes to the same origin (Caddy in production, the Vite proxy in
 * development), so the `tse_session` cookie is first-party and sent
 * automatically. Failures throw `ApiError` with a message that is safe to
 * show to the user: the server's own `error` text when it sent one, otherwise
 * a plain explanation of what to do next.
 */
import type {
  Account,
  BookSnapshot,
  CancelOrderResponse,
  Guest,
  JoinRequest,
  LeaderboardResponse,
  OpenOrdersResponse,
  SessionResponse,
  Side,
  SubmitOrderRequest,
  SubmitOrderResponse,
  SymbolSummary,
  TradesResponse,
} from "./types";

export const API_BASE = "/v1";

export class ApiError extends Error {
  /** HTTP status, or 0 when the server could not be reached. */
  readonly status: number;
  /** Seconds to wait before retrying, from `Retry-After` on a 429. */
  readonly retryAfter: number | null;

  constructor(
    status: number,
    message: string,
    retryAfter: number | null = null,
  ) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.retryAfter = retryAfter;
  }

  /** The guest session is missing or expired; the user needs to join again. */
  get isUnauthorized(): boolean {
    return this.status === 401;
  }

  get isRateLimited(): boolean {
    return this.status === 429;
  }
}

export const NETWORK_ERROR_MESSAGE =
  "Can't reach the exchange server. Check your connection and try again.";

/** Fallback text for a status when the server did not send an `error` field. */
export function fallbackMessage(
  status: number,
  retryAfter: number | null = null,
): string {
  switch (status) {
    case 400:
      return "The server rejected this request. Check the values and try again.";
    case 401:
      return "Your guest session has ended. Join again to keep trading.";
    case 404:
      return "Not found. It may have filled, been cancelled, or never existed.";
    case 409:
      return "Trading is paused for this symbol. Wait for the session to open and try again.";
    case 429:
      return retryAfter && retryAfter > 0
        ? `Too many requests. Wait ${retryAfter} seconds and try again.`
        : "Too many requests. Wait a few seconds and try again.";
    default:
      return status >= 500
        ? "The exchange server had a problem. Try again in a moment."
        : `Request failed with status ${status}. Try again.`;
  }
}

function parseRetryAfter(value: string | null): number | null {
  if (!value) return null;
  const seconds = Number(value);
  return Number.isFinite(seconds) && seconds >= 0 ? Math.ceil(seconds) : null;
}

async function readErrorMessage(res: Response): Promise<string | null> {
  try {
    const body: unknown = await res.json();
    if (body && typeof body === "object" && "error" in body) {
      const msg = (body as { error: unknown }).error;
      if (typeof msg === "string" && msg.trim()) return msg.trim();
    }
  } catch {
    // Not JSON (proxy error page, empty body). Fall back to a generic message.
  }
  return null;
}

/** Low-level request. Exported for tests and for endpoints not wrapped below. */
export async function request<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set("Accept", "application/json");
  if (init.body !== undefined && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }

  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, {
      ...init,
      headers,
      credentials: "same-origin",
    });
  } catch {
    throw new ApiError(0, NETWORK_ERROR_MESSAGE);
  }

  if (!res.ok) {
    const retryAfter = parseRetryAfter(res.headers.get("Retry-After"));
    const serverMessage = await readErrorMessage(res);
    // A 429 from the server rarely says how long to wait; the fallback does.
    const message =
      res.status === 429 && !serverMessage
        ? fallbackMessage(429, retryAfter)
        : (serverMessage ?? fallbackMessage(res.status, retryAfter));
    throw new ApiError(res.status, message, retryAfter);
  }

  try {
    return (await res.json()) as T;
  } catch {
    throw new ApiError(
      res.status,
      "The server sent a response this page can't read. Reload and try again.",
    );
  }
}

// ── Endpoints ───────────────────────────────────────────────────────────────

export function join(displayName: string): Promise<Guest> {
  const body: JoinRequest = { display_name: displayName };
  return request<Guest>("/join", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

/** The current guest, or `null` when there is no valid session (401). */
export async function getMe(): Promise<Guest | null> {
  try {
    return await request<Guest>("/me");
  } catch (err) {
    if (err instanceof ApiError && err.isUnauthorized) return null;
    throw err;
  }
}

export function submitOrder(
  order: SubmitOrderRequest,
): Promise<SubmitOrderResponse> {
  return request<SubmitOrderResponse>("/orders", {
    method: "POST",
    body: JSON.stringify(order),
  });
}

export function cancelOrder(
  orderId: string,
  symbol: string,
  side: Side,
): Promise<CancelOrderResponse> {
  const query = new URLSearchParams({ symbol, side });
  return request<CancelOrderResponse>(
    `/orders/${encodeURIComponent(orderId)}?${query.toString()}`,
    { method: "DELETE" },
  );
}

export function getBook(symbol: string): Promise<BookSnapshot> {
  return request<BookSnapshot>(`/book/${encodeURIComponent(symbol)}`);
}

export function getSession(symbol: string): Promise<SessionResponse> {
  return request<SessionResponse>(`/session/${encodeURIComponent(symbol)}`);
}

export function getSymbols(): Promise<SymbolSummary[]> {
  return request<SymbolSummary[]>("/symbols");
}

/** Recent trades, newest first. */
export function getTrades(
  symbol: string,
  limit = 500,
): Promise<TradesResponse> {
  return request<TradesResponse>(
    `/trades/${encodeURIComponent(symbol)}?limit=${limit}`,
  );
}

export function getLeaderboard(limit = 10): Promise<LeaderboardResponse> {
  return request<LeaderboardResponse>(`/leaderboard?limit=${limit}`);
}

export function getMyOrders(): Promise<OpenOrdersResponse> {
  return request<OpenOrdersResponse>("/me/orders");
}

export function getMyAccount(): Promise<Account> {
  return request<Account>("/me/account");
}
