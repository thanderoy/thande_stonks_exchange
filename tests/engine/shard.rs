use common::{OrderId, Symbol};
use engine::matcher::Trade;
use engine::order::{Order, Side, TimeInForce};
use engine::shard::{start_shard, EngineCommand};
use rust_decimal_macros::dec;
use tokio::sync::mpsc;

fn sym() -> Symbol {
    Symbol::new("TSLA")
}

async fn submit(tx: &mpsc::Sender<EngineCommand>, order: Order) -> Vec<Trade> {
    let (reply_tx, mut reply_rx) = mpsc::channel(1);
    tx.send(EngineCommand::Submit(order, reply_tx)).await.unwrap();
    reply_rx.recv().await.unwrap()
}

async fn cancel(tx: &mpsc::Sender<EngineCommand>, id: OrderId, side: Side) -> Option<Order> {
    let (reply_tx, mut reply_rx) = mpsc::channel(1);
    tx.send(EngineCommand::Cancel(id, side, reply_tx)).await.unwrap();
    reply_rx.recv().await.unwrap()
}

#[tokio::test]
async fn submit_unmatched_order_returns_no_trades() {
    let tx = start_shard(sym());
    let order = Order::new_limit(sym(), Side::Buy, dec!(100), dec!(10), TimeInForce::GoodTillCanceled, "c1");
    let trades = submit(&tx, order).await;
    assert!(trades.is_empty());
}

#[tokio::test]
async fn crossed_orders_produce_trade() {
    let tx = start_shard(sym());
    let sell = Order::new_limit(sym(), Side::Sell, dec!(100), dec!(5), TimeInForce::GoodTillCanceled, "seller");
    let buy = Order::new_limit(sym(), Side::Buy, dec!(100), dec!(5), TimeInForce::GoodTillCanceled, "buyer");
    let _ = submit(&tx, sell).await;
    let trades = submit(&tx, buy).await;
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].quantity, dec!(5));
}

#[tokio::test]
async fn cancel_resting_order_returns_it() {
    let tx = start_shard(sym());
    let order = Order::new_limit(sym(), Side::Buy, dec!(99), dec!(10), TimeInForce::GoodTillCanceled, "c1");
    let id = order.id;
    let _ = submit(&tx, order).await;
    let removed = cancel(&tx, id, Side::Buy).await;
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().id, id);
}

#[tokio::test]
async fn cancel_nonexistent_returns_none() {
    let tx = start_shard(sym());
    let result = cancel(&tx, OrderId::new(), Side::Buy).await;
    assert!(result.is_none());
}
