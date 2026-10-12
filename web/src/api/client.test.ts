import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ApiError,
  NETWORK_ERROR_MESSAGE,
  cancelOrder,
  getBook,
  getMe,
  join,
  submitOrder,
} from "./client";

function jsonResponse(
  status: number,
  body: unknown,
  headers: Record<string, string> = {},
) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", ...headers },
  });
}

const fetchMock = vi.fn<typeof fetch>();

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("request helpers", () => {
  it("posts JSON to /v1/join and returns the guest", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(200, { client_id: "c-1", display_name: "Ada" }),
    );

    const guest = await join("Ada");

    expect(guest).toEqual({ client_id: "c-1", display_name: "Ada" });
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/v1/join");
    expect(init?.method).toBe("POST");
    expect(init?.credentials).toBe("same-origin");
    expect(JSON.parse(String(init?.body))).toEqual({ display_name: "Ada" });
    expect(new Headers(init?.headers).get("Content-Type")).toBe(
      "application/json",
    );
  });

  it("sends orders without a client_id and returns trades", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(200, { order_id: "o-1", trades: [] }),
    );

    const res = await submitOrder({
      symbol: "AAPL",
      side: "Buy",
      order_type: { type: "Limit", price: "100.00" },
      quantity: "100",
      time_in_force: "GoodTillCanceled",
    });

    expect(res.order_id).toBe("o-1");
    const body = JSON.parse(String(fetchMock.mock.calls[0][1]?.body));
    expect(body).not.toHaveProperty("client_id");
    expect(body.order_type).toEqual({ type: "Limit", price: "100.00" });
  });

  it("builds the cancel URL with symbol and side", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse(200, { cancelled: "o 1" }));

    await cancelOrder("o 1", "MSFT", "Sell");

    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/v1/orders/o%201?symbol=MSFT&side=Sell");
    expect(init?.method).toBe("DELETE");
  });

  it("surfaces the server's error message", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(400, { error: "price 130 is outside the collar 80-120" }),
    );

    const err = await getBook("AAPL").catch((e: unknown) => e);

    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).status).toBe(400);
    expect((err as ApiError).message).toBe(
      "price 130 is outside the collar 80-120",
    );
  });

  it("falls back to a helpful message when the body is not JSON", async () => {
    fetchMock.mockResolvedValueOnce(
      new Response("<html>Bad Gateway</html>", { status: 502 }),
    );

    const err = (await getBook("AAPL").catch((e: unknown) => e)) as ApiError;

    expect(err.status).toBe(502);
    expect(err.message).toBe(
      "The exchange server had a problem. Try again in a moment.",
    );
  });

  it("explains how long to wait on 429", async () => {
    fetchMock.mockResolvedValueOnce(
      new Response("", { status: 429, headers: { "Retry-After": "7" } }),
    );

    const err = (await join("Ada").catch((e: unknown) => e)) as ApiError;

    expect(err.isRateLimited).toBe(true);
    expect(err.retryAfter).toBe(7);
    expect(err.message).toBe(
      "Too many requests. Wait 7 seconds and try again.",
    );
  });

  it("reports a network failure with status 0", async () => {
    fetchMock.mockRejectedValueOnce(new TypeError("Failed to fetch"));

    const err = (await getBook("AAPL").catch((e: unknown) => e)) as ApiError;

    expect(err.status).toBe(0);
    expect(err.message).toBe(NETWORK_ERROR_MESSAGE);
  });
});

describe("401 handling", () => {
  it("getMe returns null when there is no session", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse(401, { error: "no session" }));

    await expect(getMe()).resolves.toBeNull();
  });

  it("getMe returns the guest when the session is valid", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(200, { client_id: "c-9", display_name: "Grace" }),
    );

    await expect(getMe()).resolves.toEqual({
      client_id: "c-9",
      display_name: "Grace",
    });
  });

  it("getMe rethrows errors other than 401", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse(500, { error: "boom" }));

    await expect(getMe()).rejects.toMatchObject({
      status: 500,
      message: "boom",
    });
  });

  it("other endpoints throw an unauthorized ApiError on 401", async () => {
    fetchMock.mockResolvedValueOnce(new Response("", { status: 401 }));

    const err = (await submitOrder({
      symbol: "AAPL",
      side: "Sell",
      order_type: { type: "Market" },
      quantity: "1",
      time_in_force: "ImmediateOrCancel",
    }).catch((e: unknown) => e)) as ApiError;

    expect(err.isUnauthorized).toBe(true);
    expect(err.message).toBe(
      "Your guest session has ended. Join again to keep trading.",
    );
  });
});
