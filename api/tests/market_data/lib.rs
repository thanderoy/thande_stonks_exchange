use common::{OrderId, Symbol};
use engine::order::Side;
use market_data::{EventBus, MarketEvent};
use rust_decimal_macros::dec;
use serde_json::json;

#[test]
fn subscriber_receives_published_event() {
    let bus = EventBus::new(16);
    let mut rx = bus.subscribe();
    bus.publish(MarketEvent::OrderCancelled {
        order_id: OrderId::new(),
        symbol: Symbol::new("AAPL"),
        client_id: "alice".into(),
        side: Side::Buy,
        remaining: dec!(5),
    });
    let event = rx.try_recv().expect("should have received an event");
    assert!(matches!(event, MarketEvent::OrderCancelled { .. }));
}

#[test]
fn late_subscriber_misses_earlier_events() {
    let bus = EventBus::new(4);
    // Publish 5 events to overflow the capacity=4 buffer
    for _ in 0..5 {
        bus.publish(MarketEvent::SessionState {
            symbol: Symbol::new("AAPL"),
            state: "Open".to_string(),
        });
    }
    // Subscribe after publishing — should see nothing buffered beyond capacity
    let mut rx = bus.subscribe();
    // A fresh subscriber has no backlog messages
    assert!(rx.try_recv().is_err());
}

#[test]
fn order_events_are_private_and_market_events_are_public() {
    let cancelled = MarketEvent::OrderCancelled {
        order_id: OrderId::new(),
        symbol: Symbol::new("AAPL"),
        client_id: "alice".into(),
        side: Side::Sell,
        remaining: dec!(1),
    };
    assert_eq!(cancelled.owner(), Some("alice"));
    assert_eq!(MarketEvent::MarketReset.owner(), None);
}

#[test]
fn book_update_and_reset_serialize_with_event_tags() {
    let update = MarketEvent::BookUpdate {
        symbol: Symbol::new("AAPL"),
        bids: vec![(dec!(99.50), dec!(10))],
        asks: vec![],
    };
    assert_eq!(
        serde_json::to_value(&update).unwrap(),
        json!({ "event": "book_update", "symbol": "AAPL", "bids": [["99.50", "10"]], "asks": [] })
    );
    assert_eq!(
        serde_json::to_value(MarketEvent::MarketReset).unwrap(),
        json!({ "event": "market_reset" })
    );
}
