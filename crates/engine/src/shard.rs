let (tx, rx) = tokio::sync::mpsc::channel::<EngineCommand>(10_000);

tokio::spawn(async move {
    let mut book = OrderBook::new(symbol);
    while let Some(cmd) = rx.recv().await {
        match cmd {
            EngineCommand::Submit(order) => {
                let trades = match_order(&mut book, order);
                publish_trades(trades);
            }
            EngineCommand::Cancel(id, side) => {
                cancel_order(&mut book, id, side);
            }
        }
    }
});

