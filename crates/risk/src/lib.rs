pub mod circuit_breaker;

use std::collections::HashMap;
use rust_decimal::Decimal;
use thiserror::Error;
use common::Symbol;
use engine::order::{Order, OrderType, Side};

pub type ClientId = String;

#[derive(Debug)]
pub struct SymbolLimits {
    pub active: bool,
    /// Max allowed percent deviation from reference_price before an order is rejected.
    pub price_collar_pct: Decimal,
    pub reference_price: Decimal,
}

#[derive(Debug)]
pub struct ParticipantLimits {
    pub max_notional_per_order: Decimal,
    pub max_orders_per_second: u32,
    pub current_position: HashMap<Symbol, Decimal>,
    pub max_position: Decimal,
}

#[derive(Debug, Error)]
pub enum RiskError {
    #[error("symbol {0} is not active for trading")]
    SymbolNotActive(String),
    #[error("order price outside collar bounds")]
    PriceOutsideCollar,
    #[error("order notional exceeds participant limit")]
    NotionalLimitExceeded,
    #[error("order would breach position limit")]
    PositionLimitExceeded,
    #[error("unknown participant: {0}")]
    UnknownParticipant(String),
}

pub struct RiskEngine {
    pub participant_limits: HashMap<ClientId, ParticipantLimits>,
    pub symbol_limits: HashMap<Symbol, SymbolLimits>,
}

impl RiskEngine {
    pub fn check(&self, order: &Order) -> Result<(), RiskError> {
        self.check_symbol_active(order)?;
        self.check_price_collar(order)?;
        self.check_notional_limit(order)?;
        self.check_position_limits(order)?;
        Ok(())
    }

    fn check_symbol_active(&self, order: &Order) -> Result<(), RiskError> {
        match self.symbol_limits.get(&order.symbol) {
            Some(limits) if !limits.active => Err(RiskError::SymbolNotActive(order.symbol.to_string())),
            _ => Ok(()),
        }
    }

    fn check_price_collar(&self, order: &Order) -> Result<(), RiskError> {
        let limits = match self.symbol_limits.get(&order.symbol) {
            Some(l) => l,
            None => return Ok(()),
        };
        let price = match &order.order_type {
            OrderType::Limit { price } => *price,
            OrderType::StopLimit { limit_price, .. } => *limit_price,
            OrderType::Market => return Ok(()),
        };
        let deviation = ((price - limits.reference_price) / limits.reference_price * Decimal::ONE_HUNDRED).abs();
        if deviation > limits.price_collar_pct {
            Err(RiskError::PriceOutsideCollar)
        } else {
            Ok(())
        }
    }

    fn check_notional_limit(&self, order: &Order) -> Result<(), RiskError> {
        let limits = match self.participant_limits.get(&order.client_id) {
            Some(l) => l,
            None => return Err(RiskError::UnknownParticipant(order.client_id.clone())),
        };
        let price = match &order.order_type {
            OrderType::Limit { price } => *price,
            OrderType::StopLimit { limit_price, .. } => *limit_price,
            OrderType::Market => return Ok(()),
        };
        if price * order.quantity > limits.max_notional_per_order {
            Err(RiskError::NotionalLimitExceeded)
        } else {
            Ok(())
        }
    }

    fn check_position_limits(&self, order: &Order) -> Result<(), RiskError> {
        let limits = match self.participant_limits.get(&order.client_id) {
            Some(l) => l,
            None => return Err(RiskError::UnknownParticipant(order.client_id.clone())),
        };
        let current = limits.current_position.get(&order.symbol).copied().unwrap_or(Decimal::ZERO);
        let new_position = match order.side {
            Side::Buy => current + order.quantity,
            Side::Sell => current - order.quantity,
        };
        if new_position.abs() > limits.max_position {
            Err(RiskError::PositionLimitExceeded)
        } else {
            Ok(())
        }
    }
}
