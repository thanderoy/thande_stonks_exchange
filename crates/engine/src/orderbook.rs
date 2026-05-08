use std::collections::{BTreeMap, VecDeque};
use rust_decimal::Decimal;
use crate::order::Order;
use common::Symbol;

pub struct PriceLevel {
    pub orders: VecDeque<Order>,
    pub total_quantity: Decimal,
}

pub struct OrderBook {
    pub symbol: Symbol,
    pub bids: BTreeMap<Decimal, PriceLevel>,
    pub asks: BTreeMap<Decimal, PriceLevel>,
}

impl OrderBook {
    pub fn new(symbol: Symbol) -> Self {
        Self { symbol, bids: BTreeMap::new(), asks: BTreeMap::new() }
    }

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
