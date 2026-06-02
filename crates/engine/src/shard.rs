use crate::matcher::{Trade, cancel_order, match_order};
use crate::order::{Order, Side};
use crate::orderbook::OrderBook;
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
    Cancel(OrderId, Side, mpsc::Sender<Option<Order>>),
    GetBook(mpsc::Sender<BookSnapshot>),
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
                EngineCommand::Cancel(id, side, reply) => {
                    let removed = cancel_order(&mut book, id, side);
                    let _ = reply.send(removed).await;
                }
                EngineCommand::GetBook(reply) => {
                    let bids: Vec<(Decimal, Decimal)> = book
                        .bids
                        .iter()
                        .rev()
                        .take(10)
                        .map(|(price, level)| {
                            let qty: Decimal = level
                                .orders
                                .iter()
                                .map(|o| o.quantity - o.filled_quantity)
                                .sum();
                            (*price, qty)
                        })
                        .collect();
                    let asks: Vec<(Decimal, Decimal)> = book
                        .asks
                        .iter()
                        .take(10)
                        .map(|(price, level)| {
                            let qty: Decimal = level
                                .orders
                                .iter()
                                .map(|o| o.quantity - o.filled_quantity)
                                .sum();
                            (*price, qty)
                        })
                        .collect();
                    let _ = reply.send(BookSnapshot { bids, asks }).await;
                }
            }
        }
    });
    tx
}
