use std::collections::HashMap;
use common::Symbol;
use engine::order::{Order, Side, TimeInForce};
use risk::{ParticipantLimits, RiskEngine, RiskError, SymbolLimits};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn sym() -> Symbol {
    Symbol::new("AAPL")
}

fn make_engine() -> RiskEngine {
    let mut participant_limits = HashMap::new();
    participant_limits.insert(
        "client1".to_string(),
        ParticipantLimits {
            max_notional_per_order: dec!(10_000),
            max_orders_per_second: 100,
            current_position: HashMap::new(),
            max_position: dec!(1000),
        },
    );
    let mut symbol_limits = HashMap::new();
    symbol_limits.insert(
        sym(),
        SymbolLimits {
            active: true,
            price_collar_pct: dec!(10),
            reference_price: dec!(100),
        },
    );
    RiskEngine { participant_limits, symbol_limits }
}

fn buy(price: Decimal, qty: Decimal) -> Order {
    Order::new_limit(sym(), Side::Buy, price, qty, TimeInForce::GoodTillCanceled, "client1")
}

#[test]
fn valid_order_passes_all_checks() {
    assert!(make_engine().check(&buy(dec!(100), dec!(10))).is_ok());
}

#[test]
fn inactive_symbol_is_rejected() {
    let mut engine = make_engine();
    engine.symbol_limits.get_mut(&sym()).unwrap().active = false;
    assert!(matches!(engine.check(&buy(dec!(100), dec!(10))), Err(RiskError::SymbolNotActive(_))));
}

#[test]
fn price_above_collar_is_rejected() {
    assert!(matches!(make_engine().check(&buy(dec!(116), dec!(1))), Err(RiskError::PriceOutsideCollar)));
}

#[test]
fn price_below_collar_is_rejected() {
    assert!(matches!(make_engine().check(&buy(dec!(84), dec!(1))), Err(RiskError::PriceOutsideCollar)));
}

#[test]
fn price_at_collar_boundary_is_accepted() {
    // Exactly 10% above reference: deviation == collar_pct, not strictly greater → passes
    assert!(make_engine().check(&buy(dec!(110), dec!(1))).is_ok());
}

#[test]
fn notional_over_limit_is_rejected() {
    // 100 * 105 = 10_500 > 10_000
    assert!(matches!(make_engine().check(&buy(dec!(100), dec!(105))), Err(RiskError::NotionalLimitExceeded)));
}

#[test]
fn position_over_limit_is_rejected() {
    let mut engine = make_engine();
    // Raise notional ceiling so only the position check triggers at qty=1001
    engine.participant_limits.get_mut("client1").unwrap().max_notional_per_order = dec!(200_000);
    assert!(matches!(engine.check(&buy(dec!(100), dec!(1001))), Err(RiskError::PositionLimitExceeded)));
}

#[test]
fn unknown_participant_is_rejected() {
    let order = Order::new_limit(sym(), Side::Buy, dec!(100), dec!(10), TimeInForce::GoodTillCanceled, "unknown");
    assert!(matches!(make_engine().check(&order), Err(RiskError::UnknownParticipant(_))));
}

#[test]
fn market_order_bypasses_collar_and_notional_checks() {
    let order = Order::new_market(sym(), Side::Buy, dec!(10), "client1");
    assert!(make_engine().check(&order).is_ok());
}
