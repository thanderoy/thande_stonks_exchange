use common::{OrderId, Symbol};
use engine::matcher::Trade;
use engine::order::{Order, Side, TimeInForce};
use engine::shard::{EngineCommand, start_shard};
use rust_decimal_macros::dec;
use tokio::sync::mpsc;

fn sym() -> Symbol {
    Symbol::new("TSLA")
}

async fn submit(tx: &mpsc::Sender<EngineCommand>, order: Order) -> Vec<Trade> {
    let (reply_tx, mut reply_rx) = mpsc::channel(1);
    tx.send(EngineCommand::Submit(order, reply_tx))
        .await
        .unwrap();
    reply_rx.recv().await.unwrap()
}

async fn cancel(tx: &mpsc::Sender<EngineCommand>, id: OrderId, side: Side) -> Option<Order> {
    let (reply_tx, mut reply_rx) = mpsc::channel(1);
    tx.send(EngineCommand::Cancel(id, side, reply_tx))
        .await
        .unwrap();
    reply_rx.recv().await.unwrap()
}

#[tokio::test]
async fn submit_unmatched_order_returns_no_trades() {
    let tx = start_shard(sym());
    let order = Order::new_limit(
        sym(),
        Side::Buy,
        dec!(100),
        dec!(10),
        TimeInForce::GoodTillCanceled,
        "c1",
    );
    let trades = submit(&tx, order).await;
    assert!(trades.is_empty());
}

#[tokio::test]
async fn crossed_orders_produce_trade() {
    let tx = start_shard(sym());
    let sell = Order::new_limit(
        sym(),
        Side::Sell,
        dec!(100),
        dec!(5),
        TimeInForce::GoodTillCanceled,
        "seller",
    );
    let buy = Order::new_limit(
        sym(),
        Side::Buy,
        dec!(100),
        dec!(5),
        TimeInForce::GoodTillCanceled,
        "buyer",
    );
    let _ = submit(&tx, sell).await;
    let trades = submit(&tx, buy).await;
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].quantity, dec!(5));
}

#[tokio::test]
async fn cancel_resting_order_returns_it() {
    let tx = start_shard(sym());
    let order = Order::new_limit(
        sym(),
        Side::Buy,
        dec!(99),
        dec!(10),
        TimeInForce::GoodTillCanceled,
        "c1",
    );
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

async fn open_orders(tx: &mpsc::Sender<EngineCommand>, client_id: Option<&str>) -> Vec<Order> {
    let (reply_tx, mut reply_rx) = mpsc::channel(1);
    tx.send(EngineCommand::OpenOrders(
        client_id.map(str::to_string),
        reply_tx,
    ))
    .await
    .unwrap();
    reply_rx.recv().await.unwrap()
}

fn gtc(side: Side, price: rust_decimal::Decimal, client: &str) -> Order {
    Order::new_limit(
        sym(),
        side,
        price,
        dec!(10),
        TimeInForce::GoodTillCanceled,
        client,
    )
}

#[tokio::test]
async fn open_orders_lists_best_first_and_filters_by_client() {
    let tx = start_shard(sym());
    let low_bid = gtc(Side::Buy, dec!(99), "a");
    let high_bid = gtc(Side::Buy, dec!(100), "b");
    let ask = gtc(Side::Sell, dec!(101), "a");
    for o in [low_bid.clone(), high_bid.clone(), ask.clone()] {
        submit(&tx, o).await;
    }

    let all: Vec<OrderId> = open_orders(&tx, None).await.iter().map(|o| o.id).collect();
    assert_eq!(all, vec![high_bid.id, low_bid.id, ask.id]);

    let mine: Vec<OrderId> = open_orders(&tx, Some("a"))
        .await
        .iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(mine, vec![low_bid.id, ask.id]);
}

#[tokio::test]
async fn open_orders_report_remaining_quantity_after_a_partial_fill() {
    let tx = start_shard(sym());
    submit(&tx, gtc(Side::Sell, dec!(100), "maker")).await;
    let taker = Order::new_limit(
        sym(),
        Side::Buy,
        dec!(100),
        dec!(4),
        TimeInForce::ImmediateOrCancel,
        "taker",
    );
    submit(&tx, taker).await;

    let resting = open_orders(&tx, Some("maker")).await;
    assert_eq!(resting.len(), 1);
    assert_eq!(resting[0].quantity, dec!(6));
}

#[tokio::test]
async fn clear_empties_the_book() {
    let tx = start_shard(sym());
    submit(&tx, gtc(Side::Buy, dec!(99), "a")).await;
    submit(&tx, gtc(Side::Sell, dec!(101), "b")).await;

    let (reply_tx, mut reply_rx) = mpsc::channel(1);
    tx.send(EngineCommand::Clear(reply_tx)).await.unwrap();
    reply_rx.recv().await.unwrap();

    assert!(open_orders(&tx, None).await.is_empty());
}

#[tokio::test]
async fn queues_list_orders_in_fifo_order_per_level() {
    let tx = start_shard(sym());
    let first = gtc(Side::Buy, dec!(100), "a");
    let second = gtc(Side::Buy, dec!(100), "b");
    let third = gtc(Side::Buy, dec!(100), "c");
    for o in [
        first.clone(),
        second.clone(),
        third.clone(),
        gtc(Side::Buy, dec!(99), "d"),
    ] {
        submit(&tx, o).await;
    }
    submit(&tx, gtc(Side::Sell, dec!(101), "e")).await;

    let (reply, mut rx) = mpsc::channel(1);
    tx.send(EngineCommand::Queues {
        levels: 1,
        orders_per_level: 2,
        reply,
    })
    .await
    .unwrap();
    let snap = rx.recv().await.unwrap();

    assert_eq!(snap.bids.len(), 1, "only the best level");
    let best = &snap.bids[0];
    assert_eq!(
        (best.price, best.total, best.hidden),
        (dec!(100), dec!(30), 1)
    );
    let ids: Vec<OrderId> = best.orders.iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![first.id, second.id]);
    assert_eq!(snap.asks[0].orders[0].client_id, "e");
}
