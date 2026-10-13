import { describe, expect, it } from "vitest";
import {
  formatPrice,
  formatQty,
  formatTime,
  sessionLabel,
  sessionTone,
  summarizeOrderResult,
  tickDirection,
  validateDisplayName,
  validateOrderForm,
} from "./format";

describe("formatPrice", () => {
  it("shows at least two decimals without rounding", () => {
    expect(formatPrice("100")).toBe("100.00");
    expect(formatPrice("100.5")).toBe("100.50");
    expect(formatPrice("100.12500000")).toBe("100.125");
    expect(formatPrice("1234.5")).toBe("1,234.50");
    expect(formatPrice("0099.10")).toBe("99.10");
  });

  it("passes through values it can't parse", () => {
    expect(formatPrice("n/a")).toBe("n/a");
    expect(formatPrice(null)).toBe("");
  });
});

describe("formatQty", () => {
  it("drops trailing zeros and groups thousands", () => {
    expect(formatQty("1500.00000000")).toBe("1,500");
    expect(formatQty("2.50")).toBe("2.5");
    expect(formatQty("100")).toBe("100");
  });
});

describe("formatTime", () => {
  it("formats local 24-hour time", () => {
    const d = new Date(2026, 0, 2, 9, 5, 7);
    expect(formatTime(d.toISOString())).toBe("09:05:07");
    expect(formatTime("garbage")).toBe("");
  });
});

describe("tickDirection", () => {
  it("compares numerically", () => {
    expect(tickDirection("100.00", "100.5")).toBe("up");
    expect(tickDirection("100.5", "99")).toBe("down");
    expect(tickDirection("100", "100.00")).toBe("flat");
    expect(tickDirection(null, "100")).toBe("flat");
  });
});

describe("session state", () => {
  it("normalises REST and stream shapes", () => {
    expect(sessionLabel("PreMarket")).toBe("Pre-market");
    expect(sessionLabel({ Halted: { reason: "circuit_breaker" } })).toBe("Halted");
    expect(sessionLabel("Halted")).toBe("Halted");
    expect(sessionTone("Open")).toBe("open");
    expect(sessionTone("PostMarket")).toBe("auction");
    expect(sessionTone({ Halted: { reason: "x" } })).toBe("halted");
    expect(sessionTone(null)).toBe("unknown");
  });
});

describe("validateDisplayName", () => {
  it("accepts 2 to 24 allowed characters", () => {
    expect(validateDisplayName("Ada")).toBeNull();
    expect(validateDisplayName("  big_trader-7 ")).toBeNull();
  });

  it("rejects bad lengths and characters with a fix", () => {
    expect(validateDisplayName("A")).toBe("Use 2 to 24 characters.");
    expect(validateDisplayName("x".repeat(25))).toBe("Use 2 to 24 characters.");
    expect(validateDisplayName("ada!")).toBe(
      "Use only letters, digits, spaces, underscores and hyphens.",
    );
  });
});

describe("validateOrderForm", () => {
  it("accepts a valid limit order", () => {
    expect(validateOrderForm({ kind: "Limit", price: "100.25", quantity: "10" })).toEqual({});
  });

  it("enforces the 80 to 120 collar on limit prices", () => {
    expect(validateOrderForm({ kind: "Limit", price: "79.99", quantity: "10" }).price).toBe(
      "Enter a price between 80.00 and 120.00.",
    );
    expect(validateOrderForm({ kind: "Limit", price: "120", quantity: "10" })).toEqual({});
  });

  it("ignores price on market orders", () => {
    expect(validateOrderForm({ kind: "Market", price: "", quantity: "5" })).toEqual({});
  });

  it("rejects zero, empty and non-numeric quantities", () => {
    expect(validateOrderForm({ kind: "Market", price: "", quantity: "0" }).quantity).toBe(
      "Enter a quantity greater than 0.",
    );
    expect(validateOrderForm({ kind: "Market", price: "", quantity: "" }).quantity).toBe(
      "Enter a quantity.",
    );
    expect(validateOrderForm({ kind: "Limit", price: "1e2", quantity: "-1" })).toEqual({
      price: "Enter the price as a number, like 100.25.",
      quantity: "Enter the quantity as a number, like 100.",
    });
  });
});

describe("summarizeOrderResult", () => {
  it("describes a resting order", () => {
    expect(summarizeOrderResult([], "100", "Limit", "GoodTillCanceled")).toBe(
      "Order accepted. Nothing matched yet, so it is resting on the book.",
    );
  });

  it("describes a full fill with the average price", () => {
    const trades = [
      { price: "100.00", quantity: "50" },
      { price: "101.00", quantity: "50" },
    ];
    expect(summarizeOrderResult(trades, "100", "Market", "ImmediateOrCancel")).toBe(
      "Filled 100 in 2 trades at an average of 100.50.",
    );
  });

  it("describes a partial fill", () => {
    expect(
      summarizeOrderResult([{ price: "99.5", quantity: "30" }], "100", "Limit", "GoodTillCanceled"),
    ).toBe("Filled 30 of 100 in 1 trade at an average of 99.50. The rest is resting on the book.");
  });
});
