pub struct Trade {
    pub id: TradeId,
    pub symbol: Symbol,
    pub price: Decimal,
    pub quantity: Decimal,
    pub buyer_order_id: OrderId,
    pub seller_order_id: OrderId,
    pub executed_at: DateTime<Utc>,
}

pub fn match_order(book: &mut OrderBook, incoming: Order) -> Vec<Trade> {
    let mut trades = Vec::new();
    let mut remaining = incoming.clone();

    let opposite = match incoming.side {
        Side::Buy => &mut book.asks,
        Side::Sell => &mut book.bids,
    };

    // Iterator price levels from bets to worst
    let levels: Vec<Decimal> = match incoming.side {
        Side::Buy => opposite.keys().cloned().collect(),
        Side::Sell => opposite.keys().rev().cloned().collect(),
    };

    'outer: for price in levels {
        if !price_crosses(&incoming, &price) { break; }

        let level = opposite.get_mut(&price).unwrap();
        while let Some(resting) = level.orders.front_mut() {
            if remaining.quantity == Decimal::ZERO { break 'outer; }

            let fill_qty = remaining.quantity.min(resting.quantity);

            trades.push(Trade {
                id: TradeId::new(),
                symbol: incoming.symbol.clone(),
                price,
                quantity: fill_qty,
                buyer_order_id: if incoming.side == Side::Buy { incoming.id } else { resting.id },
                seller_order_id: if incoming.side == Side::Sell { incoming.id } else { resting.id },
                executed_at: Utc::now(),
            });

            remaining.quantity -= fill_qty;
            resting.quantity -= fill_qty;

            if resting.quantity == Decimal::ZERO {
                level.orders.pop_front();
            }
        }
        if level.orders.is_empty() {
            opposite.remove(&price);
        }
    }
    // Handle remainder based on TimeInForce
    if remaining.quantity > Decimal::ZERO {
        match remaining.time_in_force {
            TimeInForce::GoodTillCanceled | TimeInForce::Day => {
                add_to_book(book, remaining);
            }
            TimeInForce::ImmediateOrCancel | TimeInForce::FillOrKill => {
                // Cancel - emit CancelEvent if needed
                // TODO: implement
            }
        }
    }
    trades
}

pub fn add_to_book(book: &mut OrderBook, order: Order) {
    book.bids.entry(order.price)
}

pub fn cancel_order(book: &mut OrderBook, order_id: OrderId, side: Side) -> Option<Order> {
    let levels = match side { Side::Buy => &mut book.bids, Side::Sell => &mut book.asks};

    for (price, level) in levels.iter_mut() {
        if let Some(pos) = level.orders.iter().position(|o| o.id == order_id) {
            let removed = level.orders.remove(pos).unwrap();
            level.total_quantity -= removed.quantity;
            return Some(removed)
    }
    None
}
