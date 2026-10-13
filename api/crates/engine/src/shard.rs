use crate::matcher::{Trade, add_to_book, cancel_order, find_order, match_order, uncross};
use crate::order::{Order, Side};
use crate::orderbook::{OrderBook, PriceLevel};
use common::{OrderId, Symbol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSnapshot {
    pub bids: Vec<(Decimal, Decimal)>, // (price, remaining_qty) descending
    pub asks: Vec<(Decimal, Decimal)>, // (price, remaining_qty) ascending
}

pub enum EngineCommand {
    Submit(Order, mpsc::Sender<Vec<Trade>>),
    /// Rest an order without matching it (pre-open order collection).
    Rest(Order, mpsc::Sender<()>),
    /// Run the opening auction over whatever rested pre-open.
    Uncross(mpsc::Sender<Vec<Trade>>),
    Cancel(OrderId, Side, mpsc::Sender<Option<Order>>),
    /// Client id of a resting order, or None if it is not on the book. Read-only.
    OrderOwner(OrderId, Side, mpsc::Sender<Option<String>>),
    GetBook(mpsc::Sender<BookSnapshot>),
    /// Resting orders, bids then asks, best price and earliest first. With a client id,
    /// only that client's orders. Read-only.
    OpenOrders(Option<String>, mpsc::Sender<Vec<Order>>),
    /// Removes every resting order (market reset).
    Clear(mpsc::Sender<()>),
}

/// Top `depth` levels per side, aggregated by remaining quantity.
pub fn book_snapshot(book: &OrderBook, depth: usize) -> BookSnapshot {
    let level_qty = |level: &PriceLevel| -> Decimal {
        level
            .orders
            .iter()
            .map(|o| o.quantity - o.filled_quantity)
            .sum()
    };
    BookSnapshot {
        bids: book
            .bids
            .iter()
            .rev()
            .take(depth)
            .map(|(price, level)| (*price, level_qty(level)))
            .collect(),
        asks: book
            .asks
            .iter()
            .take(depth)
            .map(|(price, level)| (*price, level_qty(level)))
            .collect(),
    }
}

fn open_orders(book: &OrderBook, client_id: Option<&str>) -> Vec<Order> {
    book.bids
        .values()
        .rev()
        .chain(book.asks.values())
        .flat_map(|level| level.orders.iter())
        .filter(|o| client_id.is_none_or(|c| o.client_id == c))
        .cloned()
        .collect()
}

/// Spawns a per-symbol matching engine task and returns its command sender.
/// Each symbol runs in isolation — no shared state between shards.
pub fn start_shard(symbol: Symbol) -> mpsc::Sender<EngineCommand> {
    let (tx, mut rx) = mpsc::channel::<EngineCommand>(10_000);
    tokio::spawn(async move {
        let mut book = OrderBook::new(symbol);
        while let Some(cmd) = rx.recv().await {
            match cmd {
                EngineCommand::Submit(order, reply) => {
                    let trades = match_order(&mut book, order);
                    let _ = reply.send(trades).await;
                }
                EngineCommand::Rest(order, reply) => {
                    add_to_book(&mut book, order);
                    let _ = reply.send(()).await;
                }
                EngineCommand::Uncross(reply) => {
                    let trades = uncross(&mut book);
                    let _ = reply.send(trades).await;
                }
                EngineCommand::Cancel(id, side, reply) => {
                    let removed = cancel_order(&mut book, id, side);
                    let _ = reply.send(removed).await;
                }
                EngineCommand::OrderOwner(id, side, reply) => {
                    let owner = find_order(&book, id, side).map(|o| o.client_id.clone());
                    let _ = reply.send(owner).await;
                }
                EngineCommand::GetBook(reply) => {
                    let _ = reply.send(book_snapshot(&book, 10)).await;
                }
                EngineCommand::OpenOrders(client_id, reply) => {
                    let _ = reply.send(open_orders(&book, client_id.as_deref())).await;
                }
                EngineCommand::Clear(reply) => {
                    book.bids.clear();
                    book.asks.clear();
                    let _ = reply.send(()).await;
                }
            }
        }
    });
    tx
}
