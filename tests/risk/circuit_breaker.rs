use std::time::Duration;
use engine::session::{HaltReason, SessionState, TradingSession};
use risk::circuit_breaker::CircuitBreaker;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn cb(threshold: Decimal) -> CircuitBreaker {
    CircuitBreaker {
        reference_price: dec!(100),
        upper_threshold: threshold,
        lower_threshold: threshold,
        cooldown: Duration::from_secs(300),
    }
}

fn open_session() -> TradingSession {
    let mut s = TradingSession::new();
    s.open();
    s
}

#[test]
fn no_halt_when_within_threshold() {
    let mut session = open_session();
    cb(dec!(5)).check_trade(dec!(104), &mut session);
    assert!(session.can_match());
}

#[test]
fn halt_triggered_on_upward_breach() {
    let mut session = open_session();
    cb(dec!(5)).check_trade(dec!(106), &mut session);
    assert!(!session.can_match());
}

#[test]
fn halt_triggered_on_downward_breach() {
    let mut session = open_session();
    cb(dec!(5)).check_trade(dec!(94), &mut session);
    assert!(!session.can_match());
}

#[test]
fn halt_at_exact_threshold() {
    let mut session = open_session();
    cb(dec!(5)).check_trade(dec!(105), &mut session); // exactly 5% — >= triggers halt
    assert!(!session.can_match());
}

#[test]
fn halted_session_state_contains_circuit_breaker_reason() {
    let mut session = open_session();
    cb(dec!(5)).check_trade(dec!(110), &mut session);
    assert!(matches!(
        session.state,
        SessionState::Halted { reason: HaltReason::CircuitBreaker { .. } }
    ));
}
