use chrono::{DateTime, Utc};
use common::{OrderId, Symbol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type")]
pub enum OrderType {
    Market,
    Limit {
        price: Decimal,
    },
    StopLimit {
        stop_price: Decimal,
        limit_price: Decimal,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum TimeInForce {
    GoodTillCanceled,
    ImmediateOrCancel,
    FillOrKill,
    Day,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
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

impl Order {
    pub fn new_limit(
        symbol: Symbol,
        side: Side,
        price: Decimal,
        quantity: Decimal,
        tif: TimeInForce,
        client_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: OrderId::new(),
            symbol,
            side,
            order_type: OrderType::Limit { price },
            quantity,
            filled_quantity: Decimal::ZERO,
            time_in_force: tif,
            created_at: now,
            updated_at: now,
            submitted_at: now,
            cancelled_at: None,
            client_id: client_id.into(),
        }
    }

    pub fn new_market(
        symbol: Symbol,
        side: Side,
        quantity: Decimal,
        client_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: OrderId::new(),
            symbol,
            side,
            order_type: OrderType::Market,
            quantity,
            filled_quantity: Decimal::ZERO,
            time_in_force: TimeInForce::ImmediateOrCancel,
            created_at: now,
            updated_at: now,
            submitted_at: now,
            cancelled_at: None,
            client_id: client_id.into(),
        }
    }
}

/// What happened to an order once it was submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    /// Some quantity is resting on the book.
    Resting,
    /// Fully filled.
    Filled,
    /// The unfilled remainder was dropped: a market, IOC or FOK order that didn't fill completely.
    Expired,
}

impl Order {
    /// Whether an unfilled remainder of this order rests on the book.
    pub fn rests(&self) -> bool {
        !matches!(self.order_type, OrderType::Market)
            && matches!(
                self.time_in_force,
                TimeInForce::GoodTillCanceled | TimeInForce::Day
            )
    }
}
