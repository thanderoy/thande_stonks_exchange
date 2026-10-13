import {
  ApiError,
  getBook,
  getMyAccount,
  getMyOrders,
  getSymbols,
  getTrades,
} from "~/api/client";
import { SYMBOLS, type Trade } from "~/api/types";
import type { Snapshot } from "./apply-event";

/**
 * Fetch everything the app pages show, in parallel. The visitor's own orders
 * and account come back null without a guest session; a 401 there is not an
 * error. Any other failure rejects.
 */
export async function loadSnapshot(): Promise<Snapshot> {
  const optional = <T>(p: Promise<T>): Promise<T | null> =>
    p.catch((err: unknown) => {
      if (err instanceof ApiError && err.isUnauthorized) return null;
      throw err;
    });
  const [symbols, books, trades, orders, account] = await Promise.all([
    getSymbols(),
    Promise.all(SYMBOLS.map((s) => getBook(s))),
    Promise.all(SYMBOLS.map((s) => getTrades(s))),
    optional(getMyOrders()),
    optional(getMyAccount()),
  ]);
  const bySymbol: Record<string, Trade[]> = {};
  for (const t of trades) bySymbol[t.symbol] = t.trades;
  return {
    symbols,
    books,
    trades: bySymbol,
    orders: orders?.orders ?? null,
    account,
  };
}
