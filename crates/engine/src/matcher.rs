use crate::order::{Order, OrderType, Side, TimeInForce};
use crate::orderbook::{OrderBook, PriceLevel};
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
    pub executed_at: DateTime<Utc>,
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

pub fn match_order(book: &mut OrderBook, incoming: Order) -> Vec<Trade> {
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
            trades.push(Trade {
                id: TradeId::new(),
                symbol: incoming.symbol.clone(),
                price,
                quantity: fill_qty,
                buyer_order_id: if incoming.side == Side::Buy {
                    incoming.id
                } else {
                    resting.id
                },
                seller_order_id: if incoming.side == Side::Sell {
                    incoming.id
                } else {
                    resting.id
                },
                executed_at: Utc::now(),
            });
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
