import {
  ApiError,
  getBook,
  getMyAccount,
  getMyOrders,
  getSandbox,
  getSymbols,
  getTrades,
} from "~/api/client";
import type { Trade } from "~/api/types";
import type { Snapshot } from "./apply-event";

/**
 * Fetch everything the app pages show. The symbol list comes first, since it
 * includes the visitor's sandbox; then every symbol's book and trades, the
 * visitor's orders, account and sandbox, in parallel. Without a guest session
 * those last three come back null; a 401 there is not an error. Any other
 * failure rejects.
 */
export async function loadSnapshot(): Promise<Snapshot> {
  const optional = <T>(p: Promise<T>): Promise<T | null> =>
    p.catch((err: unknown) => {
      if (err instanceof ApiError && err.isUnauthorized) return null;
      throw err;
    });
  const symbols = await getSymbols();
  const names = symbols.map((s) => s.symbol);
  const [books, trades, orders, account, sandbox] = await Promise.all([
    Promise.all(names.map((s) => getBook(s))),
    Promise.all(names.map((s) => getTrades(s))),
    optional(getMyOrders()),
    optional(getMyAccount()),
    optional(getSandbox()),
  ]);
  const bySymbol: Record<string, Trade[]> = {};
  for (const t of trades) bySymbol[t.symbol] = t.trades;
  return {
    symbols,
    books,
    trades: bySymbol,
    orders: orders?.orders ?? null,
    account,
    sandbox,
  };
}
