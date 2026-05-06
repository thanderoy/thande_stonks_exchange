use rust_decimal::Decimal;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use common::{OrderId, Symbol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderType {
    Market,
    Limit {
        price: Decimal
    },
    StopLimit {
        stop_price: Decimal,
        limit_price: Decimal,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeInForce {
    GoodTillCanceled,    // GTC
    ImmediateOrCancel,   // IOC
    FillOrKill,          // FOK
    Day,                 // Expire at end of trading session
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: OrderId,
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub time_in_force: TimeInForce,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub submitted_at: DateTime<Utc>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub client_id: String,
}

