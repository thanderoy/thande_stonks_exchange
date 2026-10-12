use common::{OrderId, Symbol};
use market_data::{EventBus, MarketEvent};

#[test]
fn subscriber_receives_published_event() {
    let bus = EventBus::new(16);
    let mut rx = bus.subscribe();
    bus.publish(MarketEvent::OrderAccepted {
        order_id: OrderId::new(),
        symbol: Symbol::new("AAPL"),
    });
    let event = rx.try_recv().expect("should have received an event");
    assert!(matches!(event, MarketEvent::OrderAccepted { .. }));
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
