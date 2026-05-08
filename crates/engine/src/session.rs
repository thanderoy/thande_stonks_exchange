use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionState {
    PreMarket,
    Open,
    Halted { reason: HaltReason },
    PostMarket,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HaltReason {
    CircuitBreaker { threshold_pct: Decimal },
    RegulatoryHalt,
    TechnicalIssue,
}

pub struct TradingSession {
    pub state: SessionState,
}

impl TradingSession {
    pub fn new() -> Self {
        Self { state: SessionState::PreMarket }
    }

    pub fn can_accept_orders(&self) -> bool {
        matches!(self.state, SessionState::Open | SessionState::PreMarket)
    }

    pub fn can_match(&self) -> bool {
        self.state == SessionState::Open
    }

    pub fn open(&mut self) {
        self.state = SessionState::Open;
    }

    pub fn halt(&mut self, reason: HaltReason) {
        self.state = SessionState::Halted { reason };
    }

    pub fn resume(&mut self) {
        if matches!(self.state, SessionState::Halted { .. }) {
            self.state = SessionState::Open;
        }
    }

    pub fn close(&mut self) {
        self.state = SessionState::Closed;
    }
}

/// Finds the price that maximises matched volume across the bid/ask curves.
///
/// Each entry is `(price, cumulative_quantity)` — bids sorted descending, asks ascending.
/// Returns `None` if no price clears any volume (no crossing).
pub fn compute_auction_price(bids: &[(Decimal, Decimal)], asks: &[(Decimal, Decimal)]) -> Option<Decimal> {
    if bids.is_empty() || asks.is_empty() {
        return None;
    }

    // Candidate prices are the union of all bid and ask price levels.
    let mut candidates: Vec<Decimal> = bids.iter().map(|(p, _)| *p)
        .chain(asks.iter().map(|(p, _)| *p))
        .collect();
    candidates.sort();
    candidates.dedup();

    let mut best_volume = Decimal::ZERO;
    let mut best_prices: Vec<Decimal> = Vec::new();

    for &p in &candidates {
        // Cumulative buy demand at price p: sum of bid qty where bid_price >= p
        let demand: Decimal = bids.iter()
            .filter(|(bid_price, _)| *bid_price >= p)
            .map(|(_, qty)| *qty)
            .sum();

        // Cumulative sell supply at price p: sum of ask qty where ask_price <= p
        let supply: Decimal = asks.iter()
            .filter(|(ask_price, _)| *ask_price <= p)
            .map(|(_, qty)| *qty)
            .sum();

        let matched = demand.min(supply);

        if matched > best_volume {
            best_volume = matched;
            best_prices.clear();
            best_prices.push(p);
        } else if matched == best_volume && matched > Decimal::ZERO {
            best_prices.push(p);
        }
    }

    if best_volume == Decimal::ZERO || best_prices.is_empty() {
        return None;
    }

    // Tiebreak: price closest to the midpoint of the tied range
    if best_prices.len() == 1 {
        return Some(best_prices[0]);
    }
    let lo = *best_prices.first().unwrap();
    let hi = *best_prices.last().unwrap();
    let mid = (lo + hi) / Decimal::TWO;
    let closest = best_prices.into_iter().min_by_key(|&p| {
        let diff = (p - mid).abs();
        diff
    });
    closest
}
