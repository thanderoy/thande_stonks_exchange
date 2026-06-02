use common::{OrderId, Symbol};
use engine::matcher::{add_to_book, cancel_order, match_order};
use engine::order::{Order, Side, TimeInForce};
use engine::orderbook::OrderBook;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn sym() -> Symbol {
    Symbol::new("AAPL")
}

fn bid(price: Decimal, qty: Decimal) -> Order {
    Order::new_limit(
        sym(),
        Side::Buy,
        price,
        qty,
        TimeInForce::GoodTillCanceled,
        "buyer",
    )
}

fn ask(price: Decimal, qty: Decimal) -> Order {
    Order::new_limit(
        sym(),
        Side::Sell,
        price,
        qty,
        TimeInForce::GoodTillCanceled,
        "seller",
    )
}

fn book() -> OrderBook {
    OrderBook::new(sym())
}

// --- add_to_book ---

#[test]
fn add_bid_appears_in_bids() {
    let mut b = book();
    add_to_book(&mut b, bid(dec!(100), dec!(10)));
    assert_eq!(b.best_bid(), Some(dec!(100)));
    assert!(b.asks.is_empty());
}

#[test]
fn add_ask_appears_in_asks() {
    let mut b = book();
    add_to_book(&mut b, ask(dec!(101), dec!(5)));
    assert_eq!(b.best_ask(), Some(dec!(101)));
    assert!(b.bids.is_empty());
}

#[test]
fn market_order_not_added_to_book() {
    let mut b = book();
    add_to_book(&mut b, Order::new_market(sym(), Side::Buy, dec!(10), "c1"));
    assert!(b.bids.is_empty());
}

#[test]
fn same_price_orders_accumulate_in_level() {
    let mut b = book();
    add_to_book(&mut b, ask(dec!(100), dec!(5)));
    add_to_book(&mut b, ask(dec!(100), dec!(3)));
    let level = b.asks.get(&dec!(100)).unwrap();
    assert_eq!(level.orders.len(), 2);
    assert_eq!(level.total_quantity, dec!(8));
}

// --- cancel_order ---

#[test]
fn cancel_removes_order_and_returns_it() {
    let mut b = book();
    let o = bid(dec!(100), dec!(10));
    let id = o.id;
    add_to_book(&mut b, o);
    let removed = cancel_order(&mut b, id, Side::Buy);
    assert!(removed.is_some());
    assert!(b.bids.is_empty());
}

#[test]
fn cancel_nonexistent_returns_none() {
    let mut b = book();
    assert!(cancel_order(&mut b, OrderId::new(), Side::Buy).is_none());
}

#[test]
fn cancel_decrements_total_quantity() {
    let mut b = book();
    let o1 = bid(dec!(100), dec!(7));
    let o2 = bid(dec!(100), dec!(3));
    let id1 = o1.id;
    add_to_book(&mut b, o1);
    add_to_book(&mut b, o2);
    cancel_order(&mut b, id1, Side::Buy);
    let level = b.bids.get(&dec!(100)).unwrap();
    assert_eq!(level.total_quantity, dec!(3));
    assert_eq!(level.orders.len(), 1);
}

// --- match_order ---

#[test]
fn exact_fill_clears_ask_level() {
    let mut b = book();
    add_to_book(&mut b, ask(dec!(100), dec!(10)));
    let trades = match_order(&mut b, bid(dec!(100), dec!(10)));
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].quantity, dec!(10));
    assert_eq!(trades[0].price, dec!(100));
    assert!(b.asks.is_empty());
}

#[test]
fn partial_fill_leaves_resting_remainder() {
    let mut b = book();
    add_to_book(&mut b, ask(dec!(100), dec!(10)));
    let trades = match_order(&mut b, bid(dec!(100), dec!(6)));
    assert_eq!(trades[0].quantity, dec!(6));
    let remaining = b.asks[&dec!(100)].orders.front().unwrap().quantity;
    assert_eq!(remaining, dec!(4));
}

#[test]
fn gtc_unfilled_buy_rests_in_book() {
    let mut b = book();
    let trades = match_order(&mut b, bid(dec!(99), dec!(5)));
    assert!(trades.is_empty());
    assert_eq!(b.best_bid(), Some(dec!(99)));
}

#[test]
fn ioc_unfilled_is_discarded() {
    let mut b = book();
    let o = Order::new_limit(
        sym(),
        Side::Buy,
        dec!(100),
        dec!(10),
        TimeInForce::ImmediateOrCancel,
        "c1",
    );
    let trades = match_order(&mut b, o);
    assert!(trades.is_empty());
    assert!(b.bids.is_empty());
}

#[test]
fn buy_does_not_cross_above_limit_price() {
    let mut b = book();
    add_to_book(&mut b, ask(dec!(105), dec!(10)));
    let trades = match_order(&mut b, bid(dec!(100), dec!(10)));
    assert!(trades.is_empty());
    assert_eq!(b.best_bid(), Some(dec!(100)));
}

#[test]
fn sell_does_not_cross_below_limit_price() {
    let mut b = book();
    add_to_book(&mut b, bid(dec!(95), dec!(10)));
    let o = Order::new_limit(
        sym(),
        Side::Sell,
        dec!(100),
        dec!(10),
        TimeInForce::GoodTillCanceled,
        "s1",
    );
    let trades = match_order(&mut b, o);
    assert!(trades.is_empty());
    assert_eq!(b.best_ask(), Some(dec!(100)));
}

#[test]
fn fifo_fills_oldest_resting_order_first() {
    let mut b = book();
    let first = ask(dec!(100), dec!(3));
    let second = ask(dec!(100), dec!(7));
    let first_id = first.id;
    add_to_book(&mut b, first);
    add_to_book(&mut b, second);
    let trades = match_order(&mut b, bid(dec!(100), dec!(3)));
    assert_eq!(trades[0].seller_order_id, first_id);
}

#[test]
fn multi_level_sweep_fills_best_prices_first() {
    let mut b = book();
    add_to_book(&mut b, ask(dec!(100), dec!(5)));
    add_to_book(&mut b, ask(dec!(101), dec!(5)));
    let trades = match_order(&mut b, bid(dec!(102), dec!(10)));
    assert_eq!(trades.len(), 2);
    assert_eq!(trades[0].price, dec!(100));
    assert_eq!(trades[1].price, dec!(101));
    assert!(b.asks.is_empty());
}

#[test]
fn market_buy_fills_at_best_ask_price() {
    let mut b = book();
    add_to_book(&mut b, ask(dec!(99), dec!(10)));
    let trades = match_order(&mut b, Order::new_market(sym(), Side::Buy, dec!(5), "c1"));
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].price, dec!(99));
    assert_eq!(trades[0].quantity, dec!(5));
}

#[test]
fn buyer_and_seller_ids_assigned_correctly() {
    let mut b = book();
    let resting_ask = ask(dec!(100), dec!(10));
    let resting_id = resting_ask.id;
    add_to_book(&mut b, resting_ask);
    let incoming = bid(dec!(100), dec!(5));
    let incoming_id = incoming.id;
    let trades = match_order(&mut b, incoming);
    assert_eq!(trades[0].buyer_order_id, incoming_id);
    assert_eq!(trades[0].seller_order_id, resting_id);
}
