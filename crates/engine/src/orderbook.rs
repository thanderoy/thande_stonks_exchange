use std::collections::{BTreeMap, VecDeque};
use rust_decimal::Decimal;
use crate::order::{Order, Side};
use common::Symbol;

pub struct PriceLevel {
    pub orders: VecDeque<Order>,
    pub total_quantity: Decimal,
}

pub struct OrderBook {
    pub symbol: Symbol,
    // BTreeMap keeps keys sorted, so we get price levels in order
    // Highest bids first for buyers, lowest asks first for sellers
    pub bids: BTreeMap<Decimal, PriceLevel>,    // Buy orders
    pub asks: BTreeMap<Decimal, PriceLevel>,    // Sell orders
}

impl OrderBook {
    pub fn best_bid(&self) -> Option<Decimal> {
        self.bids.keys().next_back().cloned()
    }

    pub fn best_ask(&self) -> Option<Decimal> {
        self.asks.keys().next().cloned()
    }

    pub fn spread(&self) -> Option<Decimal> {
        Some(self.best_ask()? - self.best_bid()?)
    }
}

