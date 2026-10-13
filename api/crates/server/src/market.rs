//! Read side of the market: prices and volume per symbol, cash and positions per
//! participant, recent trades, and the coalesced `book_update` stream.
//!
//! Fills update stats and cash both live and on replay (via `AppState::record_fills`),
//! so they survive a restart. Recent trades are recorded on the live path only, because
//! replay regenerates trades with new ids and timestamps.

use crate::sandbox::Viewer;
use crate::state::{AppState, ParticipantKind};
use common::Symbol;
use engine::matcher::Trade;
use engine::order::Order;
use engine::session::SessionState;
use engine::shard::{BookSnapshot, EngineCommand};
use market_data::MarketEvent;
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use utoipa::ToSchema;

/// Trades kept per symbol for `GET /v1/trades/{symbol}`.
pub const HISTORY_LIMIT: usize = 500;

#[derive(Debug, Clone, Default)]
pub struct SymbolStats {
    /// Price of the first trade since the last reset.
    pub open: Option<Decimal>,
    pub last: Option<Decimal>,
    pub high: Option<Decimal>,
    pub low: Option<Decimal>,
    pub volume: Decimal,
    pub trades: u64,
}

#[derive(Debug, Default)]
pub struct MarketStats {
    pub symbols: HashMap<String, SymbolStats>,
    /// Net cash from fills per client: sales add, purchases subtract. Starts at zero.
    pub cash: HashMap<String, Decimal>,
    /// Recent live trades per symbol, newest first.
    pub history: HashMap<String, VecDeque<Trade>>,
}

impl MarketStats {
    /// Updates the symbol's stats, and both sides' cash when `count_cash`.
    pub fn record_fill(&mut self, trade: &Trade, count_cash: bool) {
        let stats = self
            .symbols
            .entry(trade.symbol.as_str().to_string())
            .or_default();
        stats.open.get_or_insert(trade.price);
        stats.last = Some(trade.price);
        stats.high = Some(stats.high.map_or(trade.price, |h| h.max(trade.price)));
        stats.low = Some(stats.low.map_or(trade.price, |l| l.min(trade.price)));
        stats.volume += trade.quantity;
        stats.trades += 1;

        if !count_cash {
            return;
        }
        let notional = trade.price * trade.quantity;
        *self.cash.entry(trade.buyer_client_id.clone()).or_default() -= notional;
        *self.cash.entry(trade.seller_client_id.clone()).or_default() += notional;
    }

    pub fn record_history(&mut self, trade: &Trade) {
        let history = self
            .history
            .entry(trade.symbol.as_str().to_string())
            .or_default();
        history.push_front(trade.clone());
        history.truncate(HISTORY_LIMIT);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// One of the configured symbols everyone trades.
    Shared,
    /// A guest's private sandbox.
    Sandbox,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SymbolSummary {
    pub symbol: String,
    pub kind: SymbolKind,
    pub state: SessionState,
    pub open: Option<Decimal>,
    pub last: Option<Decimal>,
    pub high: Option<Decimal>,
    pub low: Option<Decimal>,
    pub volume: Decimal,
    pub trades: u64,
    pub reference_price: Decimal,
    /// Lowest and highest limit price the price collar accepts.
    pub collar_low: Decimal,
    pub collar_high: Decimal,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Position {
    pub symbol: String,
    /// Net shares: positive is long, negative is short.
    pub quantity: Decimal,
    /// Last trade price, or the reference price before the first trade.
    pub mark: Decimal,
    pub value: Decimal,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Account {
    pub client_id: String,
    pub display_name: String,
    /// Net cash from fills since the last reset.
    pub cash: Decimal,
    /// Non-zero positions, in symbol order.
    pub positions: Vec<Position>,
    /// Cash plus positions at their marks. Everyone starts at zero.
    pub pnl: Decimal,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LeaderboardEntry {
    pub rank: usize,
    pub display_name: String,
    pub kind: ParticipantKind,
    pub pnl: Decimal,
    /// True for the participant asking, on the public listener.
    pub you: bool,
    #[serde(skip)]
    pub client_id: String,
}

impl AppState {
    pub fn mark_book_dirty(&self, symbol: &str) {
        self.dirty_books.lock().unwrap().insert(symbol.to_string());
    }

    /// Publishes a `book_update` for every symbol whose book changed since the last call.
    pub async fn publish_dirty_books(&self) {
        let dirty: Vec<String> = self.dirty_books.lock().unwrap().drain().collect();
        for symbol in dirty {
            if let Some(book) = self.book(&symbol).await {
                self.bus.publish(MarketEvent::BookUpdate {
                    symbol: Symbol::new(symbol),
                    bids: book.bids,
                    asks: book.asks,
                });
            }
        }
    }

    /// Publishes changed books every `interval`, so a busy book sends at most one update
    /// per interval.
    pub fn spawn_book_publisher(self: &Arc<Self>, interval: Duration) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                state.publish_dirty_books().await;
            }
        });
    }

    /// Top-10 snapshot of a symbol's book, or `None` for an unknown symbol.
    pub async fn book(&self, symbol: &str) -> Option<BookSnapshot> {
        let shard = self.shards.get(symbol)?.clone();
        let (tx, mut rx) = mpsc::channel(1);
        shard.send(EngineCommand::GetBook(tx)).await.ok()?;
        rx.recv().await
    }

    /// Resting orders across every symbol (shared ones first, then sandboxes),
    /// optionally only one client's.
    pub async fn open_orders(&self, client_id: Option<&str>) -> Vec<Order> {
        let mut orders = Vec::new();
        for symbol in self.all_symbols() {
            orders.extend(self.open_orders_in(&symbol, client_id).await);
        }
        orders
    }

    /// Shared symbols in configured order, then sandboxes by name.
    pub fn all_symbols(&self) -> Vec<String> {
        let mut sandboxes: Vec<String> = self.sandboxes.iter().map(|s| s.key().clone()).collect();
        sandboxes.sort();
        self.symbols.iter().cloned().chain(sandboxes).collect()
    }

    pub async fn open_orders_in(&self, symbol: &str, client_id: Option<&str>) -> Vec<Order> {
        let Some(shard) = self.shards.get(symbol).map(|s| s.clone()) else {
            return Vec::new();
        };
        let (tx, mut rx) = mpsc::channel(1);
        let command = EngineCommand::OpenOrders(client_id.map(str::to_string), tx);
        if shard.send(command).await.is_err() {
            return Vec::new();
        }
        rx.recv().await.unwrap_or_default()
    }

    /// Every symbol `viewer` can see: the shared ones, plus their own sandbox (all
    /// sandboxes for the admin).
    pub async fn symbol_summaries(&self, viewer: Viewer<'_>) -> Vec<SymbolSummary> {
        let symbols: Vec<String> = self
            .all_symbols()
            .into_iter()
            .filter(|s| self.visible_to(s, viewer))
            .collect();
        let mut out = Vec::with_capacity(symbols.len());
        for symbol in &symbols {
            let Some(session) = self.sessions.get(symbol).map(|s| s.clone()) else {
                continue;
            };
            let state = session.lock().await.state.clone();
            let stats = self
                .market
                .lock()
                .unwrap()
                .symbols
                .get(symbol)
                .cloned()
                .unwrap_or_default();
            let (reference_price, collar_pct) = self.collar(symbol);
            let band = reference_price * collar_pct / Decimal::ONE_HUNDRED;
            out.push(SymbolSummary {
                symbol: symbol.clone(),
                kind: if self.is_sandbox(symbol) {
                    SymbolKind::Sandbox
                } else {
                    SymbolKind::Shared
                },
                state,
                open: stats.open,
                last: stats.last,
                high: stats.high,
                low: stats.low,
                volume: stats.volume,
                trades: stats.trades,
                reference_price,
                collar_low: reference_price - band,
                collar_high: reference_price + band,
            });
        }
        out
    }

    /// Reference price and collar percentage for a symbol.
    pub fn collar(&self, symbol: &str) -> (Decimal, Decimal) {
        self.risk
            .lock()
            .unwrap()
            .symbol_limits
            .get(&Symbol::new(symbol))
            .map_or((Decimal::ONE_HUNDRED, Decimal::ZERO), |l| {
                (l.reference_price, l.price_collar_pct)
            })
    }

    /// Recent live trades, newest first, or `None` for an unknown symbol.
    pub fn recent_trades(&self, symbol: &str, limit: usize) -> Option<Vec<Trade>> {
        if !self.sessions.contains_key(symbol) {
            return None;
        }
        let market = self.market.lock().unwrap();
        Some(
            market
                .history
                .get(symbol)
                .map(|h| h.iter().take(limit).cloned().collect())
                .unwrap_or_default(),
        )
    }

    /// Mark price per symbol: the last trade, or the reference price before one.
    fn marks(&self) -> HashMap<String, Decimal> {
        // Read the reference prices first: the risk and market locks are never nested.
        let references: Vec<(String, Decimal)> = self
            .symbols
            .iter()
            .map(|s| (s.clone(), self.collar(s).0))
            .collect();
        let market = self.market.lock().unwrap();
        references
            .into_iter()
            .map(|(s, reference)| {
                let mark = market.symbols.get(&s).and_then(|st| st.last);
                (s, mark.unwrap_or(reference))
            })
            .collect()
    }

    /// Cash, positions and P&L of a named participant.
    pub fn account(&self, client_id: &str) -> Option<Account> {
        let info = self.participants.get(client_id)?.clone();
        let marks = self.marks();
        let cash = self
            .market
            .lock()
            .unwrap()
            .cash
            .get(client_id)
            .copied()
            .unwrap_or_default();
        let held: HashMap<Symbol, Decimal> = self
            .risk
            .lock()
            .unwrap()
            .participant_limits
            .get(client_id)
            .map(|l| l.current_position.clone())
            .unwrap_or_default();
        let positions: Vec<Position> = self
            .symbols
            .iter()
            .filter_map(|s| {
                let quantity = held.get(&Symbol::new(s.as_str())).copied()?;
                if quantity.is_zero() {
                    return None;
                }
                let mark = marks[s];
                Some(Position {
                    symbol: s.clone(),
                    quantity,
                    mark,
                    value: quantity * mark,
                })
            })
            .collect();
        let pnl = cash + positions.iter().map(|p| p.value).sum::<Decimal>();
        Some(Account {
            client_id: client_id.to_string(),
            display_name: info.display_name,
            cash,
            positions,
            pnl,
        })
    }

    /// Named participants who have traded since the last reset, best P&L first.
    pub fn leaderboard(&self, limit: usize, viewer: Option<&str>) -> Vec<LeaderboardEntry> {
        let traded: Vec<String> = {
            let market = self.market.lock().unwrap();
            self.participants
                .iter()
                .filter(|p| market.cash.contains_key(p.key()))
                .map(|p| p.key().clone())
                .collect()
        };
        let mut entries: Vec<LeaderboardEntry> = traded
            .iter()
            .filter_map(|id| {
                let account = self.account(id)?;
                let kind = self.participants.get(id)?.kind;
                Some(LeaderboardEntry {
                    rank: 0,
                    display_name: account.display_name,
                    kind,
                    pnl: account.pnl,
                    you: viewer == Some(id.as_str()),
                    client_id: id.clone(),
                })
            })
            .collect();
        entries.sort_by(|a, b| b.pnl.cmp(&a.pnl).then(a.display_name.cmp(&b.display_name)));
        entries.truncate(limit);
        for (i, e) in entries.iter_mut().enumerate() {
            e.rank = i + 1;
        }
        entries
    }
}
