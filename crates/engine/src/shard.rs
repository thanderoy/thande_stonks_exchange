use tokio::sync::mpsc;
use crate::matcher::{cancel_order, match_order, Trade};
use crate::order::{Order, Side};
use crate::orderbook::OrderBook;
use common::{OrderId, Symbol};

pub enum EngineCommand {
    Submit(Order, mpsc::Sender<Vec<Trade>>),
    Cancel(OrderId, Side, mpsc::Sender<Option<Order>>),
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
            }
        }
    });
    tx
}
