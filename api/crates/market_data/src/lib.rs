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
    /// Top levels of a book after it changed, as `[price, quantity]` pairs, best first.
    /// Changes are coalesced, so not every intermediate book is sent.
    BookUpdate {
        symbol: Symbol,
        bids: Vec<(Decimal, Decimal)>,
        asks: Vec<(Decimal, Decimal)>,
    },
    /// The shared market was reset: books, positions and trade history are empty and every
    /// symbol is back in pre-market.
    MarketReset,
}

impl MarketEvent {
    /// The participant a private event belongs to; `None` for public events.
    pub fn owner(&self) -> Option<&str> {
        match self {
            Self::OrderAccepted { client_id, .. } | Self::OrderCancelled { client_id, .. } => {
                Some(client_id)
            }
            _ => None,
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
