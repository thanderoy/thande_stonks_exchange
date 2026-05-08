use std::time::Duration;
use rust_decimal::Decimal;
use engine::session::{HaltReason, TradingSession};

pub struct CircuitBreaker {
    pub reference_price: Decimal,
    pub upper_threshold: Decimal,
    pub lower_threshold: Decimal,
    pub cooldown: Duration,
}

impl CircuitBreaker {
    pub fn check_trade(&self, trade_price: Decimal, session: &mut TradingSession) {
        let pct_change = (trade_price - self.reference_price) / self.reference_price * Decimal::ONE_HUNDRED;
        if pct_change.abs() >= self.upper_threshold {
            session.halt(HaltReason::CircuitBreaker { threshold_pct: pct_change });
        }
    }
}
