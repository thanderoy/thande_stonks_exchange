pub mod circuit_breaker;

use common::Symbol;
use engine::matcher::Trade;
use engine::order::{Order, OrderType, Side};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use thiserror::Error;

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
    #[error("order rate limit exceeded ({0} orders per second)")]
    RateLimitExceeded(u32),
}

/// Length of the per-client order-rate window.
pub const RATE_WINDOW: Duration = Duration::from_secs(1);

/// Orders counted in a client's current fixed one-second window.
#[derive(Debug, Clone, Copy)]
struct RateWindow {
    started: Instant,
    count: u32,
}

pub struct RiskEngine {
    pub participant_limits: HashMap<ClientId, ParticipantLimits>,
    pub symbol_limits: HashMap<Symbol, SymbolLimits>,
    /// Live-path order counts per client. Not replayed: rate limits only gate new orders.
    rate_windows: HashMap<ClientId, RateWindow>,
}

impl RiskEngine {
    pub fn new(
        participant_limits: HashMap<ClientId, ParticipantLimits>,
        symbol_limits: HashMap<Symbol, SymbolLimits>,
    ) -> Self {
        Self {
            participant_limits,
            symbol_limits,
            rate_windows: HashMap::new(),
        }
    }

    /// Adds a participant with a flat book. An existing participant keeps its positions.
    pub fn register_participant(
        &mut self,
        client_id: ClientId,
        max_notional_per_order: Decimal,
        max_position: Decimal,
        max_orders_per_second: u32,
    ) {
        self.participant_limits
            .entry(client_id)
            .and_modify(|l| {
                l.max_notional_per_order = max_notional_per_order;
                l.max_position = max_position;
                l.max_orders_per_second = max_orders_per_second;
            })
            .or_insert_with(|| ParticipantLimits {
                max_notional_per_order,
                max_orders_per_second,
                current_position: HashMap::new(),
                max_position,
            });
    }

    /// Live-path check: the order-rate limit as of `now`, then the static checks in
    /// [`check`](Self::check). Every call that passes the rate limit counts towards it,
    /// even if a later check rejects the order. WAL replay never calls this.
    pub fn check_live(&mut self, order: &Order, now: Instant) -> Result<(), RiskError> {
        self.check_rate(&order.client_id, now)?;
        self.check(order)
    }

    /// Counts one order for `client_id` in its fixed one-second window, rejecting it once
    /// `max_orders_per_second` orders have already been counted in that window.
    pub fn check_rate(&mut self, client_id: &str, now: Instant) -> Result<(), RiskError> {
        let max = match self.participant_limits.get(client_id) {
            Some(l) => l.max_orders_per_second,
            None => return Err(RiskError::UnknownParticipant(client_id.to_string())),
        };
        let window = self
            .rate_windows
            .entry(client_id.to_string())
            .or_insert(RateWindow {
                started: now,
                count: 0,
            });
        if now.saturating_duration_since(window.started) >= RATE_WINDOW {
            *window = RateWindow {
                started: now,
                count: 0,
            };
        }
        if window.count >= max {
            return Err(RiskError::RateLimitExceeded(max));
        }
        window.count += 1;
        Ok(())
    }

    pub fn check(&self, order: &Order) -> Result<(), RiskError> {
        self.check_symbol_active(order)?;
        self.check_price_collar(order)?;
        self.check_notional_limit(order)?;
        self.check_position_limits(order)?;
        Ok(())
    }

    /// Applies a fill to both counterparties' positions so later position checks see it.
    pub fn record_trade(&mut self, trade: &Trade) {
        self.adjust_position(&trade.buyer_client_id, &trade.symbol, trade.quantity);
        self.adjust_position(&trade.seller_client_id, &trade.symbol, -trade.quantity);
    }

    fn adjust_position(&mut self, client_id: &str, symbol: &Symbol, delta: Decimal) {
        if let Some(limits) = self.participant_limits.get_mut(client_id) {
            *limits
                .current_position
                .entry(symbol.clone())
                .or_insert(Decimal::ZERO) += delta;
        }
    }

    fn check_symbol_active(&self, order: &Order) -> Result<(), RiskError> {
        match self.symbol_limits.get(&order.symbol) {
            Some(limits) if !limits.active => {
                Err(RiskError::SymbolNotActive(order.symbol.to_string()))
            }
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
        let deviation = ((price - limits.reference_price) / limits.reference_price
            * Decimal::ONE_HUNDRED)
            .abs();
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
        let current = limits
            .current_position
            .get(&order.symbol)
            .copied()
            .unwrap_or(Decimal::ZERO);
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
