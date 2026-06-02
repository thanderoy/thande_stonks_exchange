use common::Symbol;
use engine::matcher::add_to_book;
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
        "c1",
    )
}

fn ask(price: Decimal, qty: Decimal) -> Order {
    Order::new_limit(
        sym(),
        Side::Sell,
        price,
        qty,
        TimeInForce::GoodTillCanceled,
        "c1",
    )
}

#[test]
fn empty_book_returns_none() {
    let book = OrderBook::new(sym());
    assert!(book.best_bid().is_none());
    assert!(book.best_ask().is_none());
    assert!(book.spread().is_none());
}

#[test]
fn best_bid_is_highest_price() {
    let mut book = OrderBook::new(sym());
    add_to_book(&mut book, bid(dec!(99), dec!(10)));
    add_to_book(&mut book, bid(dec!(100), dec!(5)));
    add_to_book(&mut book, bid(dec!(98), dec!(15)));
    assert_eq!(book.best_bid(), Some(dec!(100)));
}

#[test]
fn best_ask_is_lowest_price() {
    let mut book = OrderBook::new(sym());
    add_to_book(&mut book, ask(dec!(102), dec!(5)));
    add_to_book(&mut book, ask(dec!(100), dec!(8)));
    add_to_book(&mut book, ask(dec!(101), dec!(10)));
    assert_eq!(book.best_ask(), Some(dec!(100)));
}

#[test]
fn spread_is_ask_minus_bid() {
    let mut book = OrderBook::new(sym());
    add_to_book(&mut book, bid(dec!(99), dec!(10)));
    add_to_book(&mut book, ask(dec!(101), dec!(10)));
    assert_eq!(book.spread(), Some(dec!(2)));
}

#[test]
fn spread_none_when_one_side_empty() {
    let mut book = OrderBook::new(sym());
    add_to_book(&mut book, bid(dec!(99), dec!(10)));
    assert!(book.spread().is_none());
}

#[test]
fn total_quantity_tracked_per_level() {
    let mut book = OrderBook::new(sym());
    add_to_book(&mut book, bid(dec!(100), dec!(5)));
    add_to_book(&mut book, bid(dec!(100), dec!(3)));
    let level = book.bids.get(&dec!(100)).unwrap();
    assert_eq!(level.total_quantity, dec!(8));
    assert_eq!(level.orders.len(), 2);
}
