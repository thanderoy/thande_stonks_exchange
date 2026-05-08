use chrono::Utc;
use common::{OrderId, Symbol};
use engine::order::{Order, OrderType, Side, TimeInForce};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn sym() -> Symbol {
    Symbol::new("AAPL")
}

#[test]
fn limit_order_fields() {
    let o = Order::new_limit(sym(), Side::Buy, dec!(100.00), dec!(10), TimeInForce::GoodTillCanceled, "c1");
    assert_eq!(o.side, Side::Buy);
    assert_eq!(o.quantity, dec!(10));
    assert_eq!(o.filled_quantity, Decimal::ZERO);
    assert!(o.cancelled_at.is_none());
    assert_eq!(o.client_id, "c1");
    assert_eq!(o.order_type, OrderType::Limit { price: dec!(100.00) });
}

#[test]
fn market_order_defaults_to_ioc() {
    let o = Order::new_market(sym(), Side::Sell, dec!(5), "c2");
    assert_eq!(o.order_type, OrderType::Market);
    assert_eq!(o.time_in_force, TimeInForce::ImmediateOrCancel);
}

#[test]
fn each_order_gets_unique_id() {
    let a = Order::new_limit(sym(), Side::Buy, dec!(100), dec!(1), TimeInForce::GoodTillCanceled, "c1");
    let b = Order::new_limit(sym(), Side::Buy, dec!(100), dec!(1), TimeInForce::GoodTillCanceled, "c1");
    assert_ne!(a.id, b.id);
}

#[test]
fn stop_limit_order_fields() {
    let now = Utc::now();
    let o = Order {
        id: OrderId::new(),
        symbol: sym(),
        side: Side::Buy,
        order_type: OrderType::StopLimit { stop_price: dec!(95), limit_price: dec!(94) },
        quantity: dec!(10),
        filled_quantity: Decimal::ZERO,
        time_in_force: TimeInForce::GoodTillCanceled,
        created_at: now,
        updated_at: now,
        submitted_at: now,
        cancelled_at: None,
        client_id: "c3".to_string(),
    };
    assert_eq!(o.order_type, OrderType::StopLimit { stop_price: dec!(95), limit_price: dec!(94) });
}
