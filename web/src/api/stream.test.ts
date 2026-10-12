import { afterEach, describe, expect, it, vi } from "vitest";
import {
  backoffDelay,
  connectStream,
  parseStreamMessage,
  streamUrl,
  type StreamStatus,
} from "./stream";

describe("backoffDelay", () => {
  it("doubles from the base and caps at the maximum", () => {
    const noJitter = () => 0;
    expect(backoffDelay(0, 500, 15_000, noJitter)).toBe(500);
    expect(backoffDelay(1, 500, 15_000, noJitter)).toBe(1000);
    expect(backoffDelay(3, 500, 15_000, noJitter)).toBe(4000);
    expect(backoffDelay(10, 500, 15_000, noJitter)).toBe(15_000);
  });

  it("subtracts at most 25% jitter", () => {
    expect(backoffDelay(2, 500, 15_000, () => 1)).toBe(1500);
  });
});

describe("streamUrl", () => {
  it("uses wss on https and ws on http", () => {
    expect(streamUrl({ protocol: "https:", host: "tse.example" })).toBe(
      "wss://tse.example/v1/stream",
    );
    expect(streamUrl({ protocol: "http:", host: "localhost:5173" })).toBe(
      "ws://localhost:5173/v1/stream",
    );
  });
});

describe("parseStreamMessage", () => {
  it("parses newline-delimited events and drops junk", () => {
    const data = [
      '{"event":"order_accepted","order_id":"o1","symbol":"AAPL"}',
      "not json",
      '{"event":"mystery"}',
      '{"event":"session_state","symbol":"AAPL","state":"Open"}',
      "",
    ].join("\n");
    const events = parseStreamMessage(data);
    expect(events.map((e) => e.event)).toEqual([
      "order_accepted",
      "session_state",
    ]);
  });
});

class FakeSocket {
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((m: { data: unknown }) => void) | null = null;
  closed = false;
  close() {
    this.closed = true;
  }
}

describe("connectStream", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("reconnects with backoff after a drop and resets after open", () => {
    vi.useFakeTimers();
    const sockets: FakeSocket[] = [];
    const statuses: StreamStatus[] = [];
    const events: string[] = [];

    const handle = connectStream("ws://x/v1/stream", {
      onEvent: (e) => events.push(e.event),
      onStatus: (s) => statuses.push(s),
      random: () => 0,
      createSocket: () => {
        const s = new FakeSocket();
        sockets.push(s);
        return s as unknown as WebSocket;
      },
    });

    expect(sockets).toHaveLength(1);
    sockets[0].onopen?.();
    sockets[0].onmessage?.({
      data: '{"event":"order_cancelled","order_id":"o","symbol":"AAPL"}',
    });
    expect(events).toEqual(["order_cancelled"]);

    sockets[0].onclose?.();
    vi.advanceTimersByTime(499);
    expect(sockets).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(2);

    // Second drop without an open in between waits twice as long.
    sockets[1].onclose?.();
    vi.advanceTimersByTime(999);
    expect(sockets).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(3);

    handle.close();
    expect(sockets[2].closed).toBe(true);
    vi.advanceTimersByTime(60_000);
    expect(sockets).toHaveLength(3);
    expect(statuses[0]).toBe("connecting");
    expect(statuses).toContain("open");
    expect(statuses).toContain("reconnecting");
    expect(statuses.at(-1)).toBe("closed");
  });
});
