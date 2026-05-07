#[derive{Debug, Clone, PartialEq}]
pub enum SessionState {
    PreMarket,
    Open,
    Halted { reason: HaltReason },
    PostMarket,
    Closed,
}

#[derive{Debug, Clone, PartialEq}]
pub enum HaltReason {
    CircuitBreaker { threshold_pct: Decimal},
    RegulatoryHalt,
    TechnicalIssue,
}

pub struct TradingSession {
    pub state: SessionState,
    pub open_time: NaiveTime,
    pub close_time: NaiveTime,
    pub timezone: Tz,
}

impl TradingSession {
    pub fn can_accept_orders(&self) -> bool {
        matches!(self.state, SessionState::Open | SessionState::PreMarket)
    }

    pub fn can_match(&self) -> bool {
        self.state == SessionState::Open
    }
}


// Find the price that maximises the order volume (number of shares matched)
pub fn compute_auction_price(book: &OrderBook) -> Option<Decimal> {
    // Build cumulative supply v. demand curves, find intersection
    todo!();
}



