use common::{OrderId, Symbol};
use engine::matcher::Trade;
use engine::order::{OrderStatus, OrderType, Side, TimeInForce};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum MarketEvent {
    Trade(Trade),
    /// Private: the public stream sends it only to `client_id`. `remaining` is what's
    /// left after the order's immediate fills, whose `trade` events come first.
    OrderAccepted {
        order_id: OrderId,
        symbol: Symbol,
        client_id: String,
        side: Side,
        order_type: OrderType,
        quantity: Decimal,
        time_in_force: TimeInForce,
        remaining: Decimal,
        status: OrderStatus,
    },
    /// Private: the public stream sends it only to `client_id`.
    OrderCancelled {
        order_id: OrderId,
        symbol: Symbol,
        client_id: String,
        side: Side,
        /// Quantity that was still resting when it was cancelled.
        remaining: Decimal,
    },
    SessionState {
        symbol: Symbol,
        state: String,
    },
    /// Public: what the engine did with one order, for the engine view. Names the
    /// participant by display name (or bot / pre-registered id), never a guest's client id.
    OrderActivity {
        order_id: OrderId,
        symbol: Symbol,
        participant: String,
        side: Side,
        /// Limit price; absent for a market order.
        #[serde(skip_serializing_if = "Option::is_none")]
        price: Option<Decimal>,
        quantity: Decimal,
        /// Left on the book (`rested`) or dropped (`expired`, `killed`, `cancelled`).
        remaining: Decimal,
        /// Trades the order took part in when it arrived.
        fills: usize,
        action: Activity,
    },
    /// Public: the opening auction matched `volume` shares in `trades` trades at `price`.
    Auction {
        symbol: Symbol,
        price: Decimal,
        volume: Decimal,
        trades: usize,
    },
    /// Top levels of a book after it changed, as `[price, quantity]` pairs, best first.
    /// Changes are coalesced, so not every intermediate book is sent.
    BookUpdate {
        symbol: Symbol,
        bids: Vec<(Decimal, Decimal)>,
        asks: Vec<(Decimal, Decimal)>,
    },
    /// Private to `client_id`: their sandbox symbol was removed.
    SandboxRemoved {
        symbol: Symbol,
        client_id: String,
        /// `deleted` (by its owner) or `idle`.
        reason: String,
    },
    /// The shared market was reset: books, positions and trade history are empty and every
    /// symbol is back in pre-market.
    MarketReset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Some or all of it is resting on the book.
    Rested,
    /// Filled completely on arrival.
    Filled,
    /// A market or IOC order whose unfilled remainder was dropped.
    Expired,
    /// A fill-or-kill order that couldn't fill completely, so nothing traded.
    Killed,
    /// A resting order was cancelled.
    Cancelled,
}

impl MarketEvent {
    /// The participant a private event belongs to; `None` for public events.
    pub fn owner(&self) -> Option<&str> {
        match self {
            Self::OrderAccepted { client_id, .. }
            | Self::OrderCancelled { client_id, .. }
            | Self::SandboxRemoved { client_id, .. } => Some(client_id),
            _ => None,
        }
    }

    /// The symbol an event is about; `None` for market-wide events.
    pub fn symbol(&self) -> Option<&str> {
        match self {
            Self::Trade(t) => Some(t.symbol.as_str()),
            Self::OrderAccepted { symbol, .. }
            | Self::OrderCancelled { symbol, .. }
            | Self::SessionState { symbol, .. }
            | Self::OrderActivity { symbol, .. }
            | Self::Auction { symbol, .. }
            | Self::BookUpdate { symbol, .. }
            | Self::SandboxRemoved { symbol, .. } => Some(symbol.as_str()),
            Self::MarketReset => None,
        }
    }
}

pub struct EventBus {
    tx: broadcast::Sender<MarketEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: MarketEvent) {
        // Ignore send errors — no active subscribers is not a failure.
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MarketEvent> {
        self.tx.subscribe()
    }
}
