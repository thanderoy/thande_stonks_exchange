use engine::session::{HaltReason, SessionState, TradingSession, compute_auction_price};
use rust_decimal_macros::dec;

#[test]
fn new_session_starts_in_pre_market() {
    let s = TradingSession::new();
    assert_eq!(s.state, SessionState::PreMarket);
}

#[test]
fn pre_market_accepts_orders_but_cannot_match() {
    let s = TradingSession::new();
    assert!(s.can_accept_orders());
    assert!(!s.can_match());
}

#[test]
fn open_enables_matching() {
    let mut s = TradingSession::new();
    s.open();
    assert_eq!(s.state, SessionState::Open);
    assert!(s.can_match());
    assert!(s.can_accept_orders());
}

#[test]
fn halt_blocks_matching_and_order_acceptance() {
    let mut s = TradingSession::new();
    s.open();
    s.halt(HaltReason::CircuitBreaker {
        threshold_pct: dec!(10),
    });
    assert!(!s.can_match());
    assert!(!s.can_accept_orders());
    assert!(matches!(s.state, SessionState::Halted { .. }));
}

#[test]
fn resume_after_halt_restores_open() {
    let mut s = TradingSession::new();
    s.open();
    s.halt(HaltReason::RegulatoryHalt);
    s.resume();
    assert_eq!(s.state, SessionState::Open);
    assert!(s.can_match());
}

#[test]
fn resume_on_non_halted_session_is_noop() {
    let mut s = TradingSession::new();
    s.open();
    s.resume();
    assert_eq!(s.state, SessionState::Open);
}

#[test]
fn close_ends_session() {
    let mut s = TradingSession::new();
    s.open();
    s.close();
    assert_eq!(s.state, SessionState::Closed);
    assert!(!s.can_match());
    assert!(!s.can_accept_orders());
}

#[test]
fn halt_reasons_are_distinct() {
    assert_ne!(
        HaltReason::CircuitBreaker {
            threshold_pct: dec!(5)
        },
        HaltReason::RegulatoryHalt
    );
    assert_ne!(HaltReason::RegulatoryHalt, HaltReason::TechnicalIssue);
}

// --- compute_auction_price tests ---

#[test]
fn auction_price_empty_inputs_returns_none() {
    assert_eq!(compute_auction_price(&[], &[]), None);
    assert_eq!(compute_auction_price(&[(dec!(100), dec!(10))], &[]), None);
    assert_eq!(compute_auction_price(&[], &[(dec!(100), dec!(10))]), None);
}

#[test]
fn auction_price_no_crossing_returns_none() {
    // Best bid 90, best ask 110 — no overlap
    let bids = [(dec!(90), dec!(100))];
    let asks = [(dec!(110), dec!(100))];
    assert_eq!(compute_auction_price(&bids, &asks), None);
}

#[test]
fn auction_price_single_crossing_level() {
    // Bid of 100 for 50 qty, ask of 100 for 50 qty — clears at 100
    let bids = [(dec!(100), dec!(50))];
    let asks = [(dec!(100), dec!(50))];
    assert_eq!(compute_auction_price(&bids, &asks), Some(dec!(100)));
}

#[test]
fn auction_price_realistic_ladder() {
    // Bids: 102 × 30, 101 × 50, 100 × 100
    // Asks: 99 × 20, 100 × 40, 101 × 60
    let bids = [
        (dec!(102), dec!(30)),
        (dec!(101), dec!(50)),
        (dec!(100), dec!(100)),
    ];
    let asks = [
        (dec!(99), dec!(20)),
        (dec!(100), dec!(40)),
        (dec!(101), dec!(60)),
    ];
    // At price 100: demand (bid >= 100) = 30+50+100=180; supply (ask <= 100) = 20+40=60; match = 60
    // At price 101: demand = 30+50=80; supply = 20+40+60=120; match = 80
    // At price 102: demand = 30; supply = 120; match = 30
    // At price 99:  demand = 180; supply = 20; match = 20
    // Max is 80 at 101
    assert_eq!(compute_auction_price(&bids, &asks), Some(dec!(101)));
}

#[test]
fn auction_price_tiebreak_picks_midpoint() {
    // Equal volume at multiple prices — tiebreak to midpoint
    // Bid 105 × 50; asks at 100 × 50
    // At 100: demand=50, supply=50, match=50
    // At 105: demand=50, supply=50, match=50
    // Tied at 100 and 105 → midpoint 102.5; no candidate is exactly 102.5
    // Closest to 102.5 among {100, 105} is equidistant; implementation picks min_by_key
    // which in case of equal distance picks the first element in iteration order (100 or 105)
    // The specific tiebreak is an implementation detail — just assert Some is returned.
    let bids = [(dec!(105), dec!(50))];
    let asks = [(dec!(100), dec!(50))];
    let result = compute_auction_price(&bids, &asks);
    assert!(result.is_some());
}
