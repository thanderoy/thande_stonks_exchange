use tokio::sync::broadcast;
use serde::{Deserialize, Serialize};
use common::{OrderId, Symbol};
use engine::matcher::Trade;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum MarketEvent {
    Trade(Trade),
    OrderAccepted { order_id: OrderId, symbol: Symbol },
    OrderCancelled { order_id: OrderId, symbol: Symbol },
    SessionState { symbol: Symbol, state: String },
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
