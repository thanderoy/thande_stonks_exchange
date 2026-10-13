use crate::order::{Order, OrderStatus, OrderType, Side, TimeInForce};
use crate::orderbook::{OrderBook, PriceLevel};
use crate::session::compute_auction_price;
use chrono::{DateTime, Utc};
use common::{OrderId, Symbol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(value_type = String, format = Uuid)]
pub struct TradeId(pub Uuid);

impl TradeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Trade {
    pub id: TradeId,
    pub symbol: Symbol,
    pub price: Decimal,
    pub quantity: Decimal,
    pub buyer_order_id: OrderId,
    pub seller_order_id: OrderId,
    pub buyer_client_id: String,
    pub seller_client_id: String,
    pub executed_at: DateTime<Utc>,
}

fn new_trade(
    symbol: &Symbol,
    price: Decimal,
    quantity: Decimal,
    buy: &Order,
    sell: &Order,
) -> Trade {
    Trade {
        id: TradeId::new(),
        symbol: symbol.clone(),
        price,
        quantity,
        buyer_order_id: buy.id,
        seller_order_id: sell.id,
        buyer_client_id: buy.client_id.clone(),
        seller_client_id: sell.client_id.clone(),
        executed_at: Utc::now(),
    }
}

/// Unfilled quantity of `order` after `trades` (the trades its submission produced), and
/// what became of the remainder.
pub fn fill_outcome(order: &Order, trades: &[Trade]) -> (Decimal, OrderStatus) {
    let filled: Decimal = trades
        .iter()
        .filter(|t| t.buyer_order_id == order.id || t.seller_order_id == order.id)
        .map(|t| t.quantity)
        .sum();
    let remaining = order.quantity - filled;
    let status = if remaining.is_zero() {
        OrderStatus::Filled
    } else if order.rests() {
        OrderStatus::Resting
    } else {
        OrderStatus::Expired
    };
    (remaining, status)
}

fn price_crosses(incoming: &Order, level_price: Decimal) -> bool {
    match &incoming.order_type {
        OrderType::Market => true,
        OrderType::Limit { price } => match incoming.side {
            Side::Buy => level_price <= *price,
            Side::Sell => level_price >= *price,
        },
        OrderType::StopLimit { limit_price, .. } => match incoming.side {
            Side::Buy => level_price <= *limit_price,
            Side::Sell => level_price >= *limit_price,
        },
    }
}

/// Resting quantity on the opposite side at prices the incoming order would trade at.
fn crossing_quantity(book: &OrderBook, incoming: &Order) -> Decimal {
    let levels: Box<dyn Iterator<Item = (&Decimal, &PriceLevel)>> = match incoming.side {
        Side::Buy => Box::new(book.asks.iter()),
        Side::Sell => Box::new(book.bids.iter().rev()),
    };
    levels
        .take_while(|(price, _)| price_crosses(incoming, **price))
        .map(|(_, level)| level.total_quantity)
        .sum()
}

pub fn match_order(book: &mut OrderBook, incoming: Order) -> Vec<Trade> {
    // Fill-or-kill is all-or-nothing: leave the book untouched unless it can fill completely.
    if incoming.time_in_force == TimeInForce::FillOrKill
        && crossing_quantity(book, &incoming) < incoming.quantity
    {
        return Vec::new();
    }

    let mut trades = Vec::new();
    let mut remaining_qty = incoming.quantity;

    // Snapshot price levels — bids descending (best first), asks ascending (best first)
    let levels: Vec<Decimal> = match incoming.side {
        Side::Buy => book.asks.keys().cloned().collect(),
        Side::Sell => book.bids.keys().rev().cloned().collect(),
    };

    for price in &levels {
        let price = *price;
        if remaining_qty == Decimal::ZERO {
            break;
        }
        if !price_crosses(&incoming, price) {
            break;
        }

        let level = match incoming.side {
            Side::Buy => book.asks.get_mut(&price),
            Side::Sell => book.bids.get_mut(&price),
        };
        let level = match level {
            Some(l) => l,
            None => continue,
        };

        while remaining_qty > Decimal::ZERO {
            let resting = match level.orders.front_mut() {
                Some(r) => r,
                None => break,
            };
            let fill_qty = remaining_qty.min(resting.quantity);
            let (buy, sell) = match incoming.side {
                Side::Buy => (&incoming, &*resting),
                Side::Sell => (&*resting, &incoming),
            };
            trades.push(new_trade(&incoming.symbol, price, fill_qty, buy, sell));
            remaining_qty -= fill_qty;
            resting.quantity -= fill_qty;
            level.total_quantity -= fill_qty;
            if resting.quantity == Decimal::ZERO {
                level.orders.pop_front();
            }
        }
    }

    // Remove depleted price levels
    for price in &levels {
        let is_empty = match incoming.side {
            Side::Buy => book.asks.get(price).map_or(false, |l| l.orders.is_empty()),
            Side::Sell => book.bids.get(price).map_or(false, |l| l.orders.is_empty()),
        };
        if is_empty {
            match incoming.side {
                Side::Buy => book.asks.remove(price),
                Side::Sell => book.bids.remove(price),
            };
        }
    }

    // Place unfilled remainder based on TIF
    if remaining_qty > Decimal::ZERO {
        let mut remainder = incoming.clone();
        remainder.quantity = remaining_qty;
        match remainder.time_in_force {
            TimeInForce::GoodTillCanceled | TimeInForce::Day => add_to_book(book, remainder),
            TimeInForce::ImmediateOrCancel | TimeInForce::FillOrKill => {}
        }
    }

    trades
}

/// Opening auction: executes every crossing order at the single price that maximises matched
/// volume, in price-time priority, leaving the book uncrossed. No-op on an uncrossed book.
pub fn uncross(book: &mut OrderBook) -> Vec<Trade> {
    let bids: Vec<(Decimal, Decimal)> = book
        .bids
        .iter()
        .rev()
        .map(|(price, level)| (*price, level.total_quantity))
        .collect();
    let asks: Vec<(Decimal, Decimal)> = book
        .asks
        .iter()
        .map(|(price, level)| (*price, level.total_quantity))
        .collect();
    let Some(auction_price) = compute_auction_price(&bids, &asks) else {
        return Vec::new();
    };

    let OrderBook { symbol, bids, asks } = book;
    let mut trades = Vec::new();
    while let (Some((&bid_px, bid_level)), Some((&ask_px, ask_level))) =
        (bids.iter_mut().next_back(), asks.iter_mut().next())
    {
        if bid_px < auction_price || ask_px > auction_price {
            break;
        }
        let (Some(buy), Some(sell)) = (bid_level.orders.front_mut(), ask_level.orders.front_mut())
        else {
            break;
        };

        let fill_qty = buy.quantity.min(sell.quantity);
        trades.push(new_trade(symbol, auction_price, fill_qty, buy, sell));
        buy.quantity -= fill_qty;
        sell.quantity -= fill_qty;
        let (buy_done, sell_done) = (buy.quantity.is_zero(), sell.quantity.is_zero());
        bid_level.total_quantity -= fill_qty;
        ask_level.total_quantity -= fill_qty;
        if buy_done {
            bid_level.orders.pop_front();
        }
        if sell_done {
            ask_level.orders.pop_front();
        }
        if bid_level.orders.is_empty() {
            bids.remove(&bid_px);
        }
        if ask_level.orders.is_empty() {
            asks.remove(&ask_px);
        }
    }
    trades
}

pub fn add_to_book(book: &mut OrderBook, order: Order) {
    let price = match &order.order_type {
        OrderType::Limit { price } => *price,
        OrderType::StopLimit { limit_price, .. } => *limit_price,
        OrderType::Market => return, // market orders never rest in the book
    };
    let qty = order.quantity;
    let levels = match order.side {
        Side::Buy => &mut book.bids,
        Side::Sell => &mut book.asks,
    };
    let level = levels.entry(price).or_insert_with(|| PriceLevel {
        orders: VecDeque::new(),
        total_quantity: Decimal::ZERO,
    });
    level.total_quantity += qty;
    level.orders.push_back(order);
}

/// The resting order with this id on the given side, if any.
pub fn find_order(book: &OrderBook, order_id: OrderId, side: Side) -> Option<&Order> {
    let levels = match side {
        Side::Buy => &book.bids,
        Side::Sell => &book.asks,
    };
    levels
        .values()
        .find_map(|level| level.orders.iter().find(|o| o.id == order_id))
}

pub fn cancel_order(book: &mut OrderBook, order_id: OrderId, side: Side) -> Option<Order> {
    let levels = match side {
        Side::Buy => &mut book.bids,
        Side::Sell => &mut book.asks,
    };
    let mut empty_price = None;
    let mut removed_order = None;
    for (price, level) in levels.iter_mut() {
        if let Some(pos) = level.orders.iter().position(|o| o.id == order_id) {
            let removed = level.orders.remove(pos).unwrap();
            level.total_quantity -= removed.quantity;
            if level.orders.is_empty() {
                empty_price = Some(*price);
            }
            removed_order = Some(removed);
            break;
        }
    }
    if let Some(price) = empty_price {
        match side {
            Side::Buy => book.bids.remove(&price),
            Side::Sell => book.asks.remove(&price),
        };
    }
    removed_order
}
