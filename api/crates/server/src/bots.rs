//! In-process bots that keep the shared market moving. They trade through the same
//! `AppState::submit` and `cancel` as everyone else, so their orders pass the session
//! gate and risk checks, go to the WAL and replay like any other.
//!
//! Per symbol, each tick:
//! - The **maker** keeps a ladder of resting bids and asks on a fixed price grid around
//!   a fair value that random-walks back towards the reference price. The ladder only
//!   moves once fair value has drifted two steps, and then only levels that leave it are
//!   cancelled, which keeps WAL growth down.
//! - The **taker** sometimes sends an IOC order that crosses the spread, so trades print.
//! - In pre-market the taker leaves a couple of crossing orders for the opening auction,
//!   and once the symbol has been in pre-market for `pre_open` the bot opens it. A halted
//!   or closed symbol is left alone.

use crate::state::AppState;
use common::Symbol;
use engine::order::{Order, OrderType, Side, TimeInForce};
use engine::session::SessionState;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub const MAKER_ID: &str = "bot-maker";
pub const TAKER_ID: &str = "bot-taker";

/// Distance between maker price levels.
const GRID: f64 = 0.05;
const LEVELS: usize = 4;
/// Gap between the ladder's centre and each side's first level, in grid steps.
const HALF_SPREAD_STEPS: f64 = 1.0;
/// The ladder only re-centres once fair value is this many grid steps from its centre.
const RECENTRE_STEPS: f64 = 2.0;
/// Per-tick random-walk size as a fraction of the price, and pull towards the reference.
const VOLATILITY: f64 = 0.0004;
const MEAN_REVERSION: f64 = 0.01;
/// Fair value stays within this fraction of the reference, well inside the collar.
const MAX_DRIFT: f64 = 0.12;
const TAKE_PROBABILITY: f64 = 0.6;

#[derive(Debug, Clone, Copy)]
pub struct BotConfig {
    /// Average time between ticks; each tick waits 70–130% of it.
    pub interval: Duration,
    /// How long a symbol stays in pre-market before the bot opens it.
    pub pre_open: Duration,
}

/// Registers the bot participants. Call at every startup before WAL replay, whether or
/// not bots will run, so replayed bot orders find their participant.
pub fn register(state: &AppState) {
    let huge = Decimal::from(1_000_000_000u64);
    state.register_bot(MAKER_ID, "Maker bot", huge, huge, 1_000);
    state.register_bot(TAKER_ID, "Taker bot", huge, huge, 1_000);
}

/// Starts one bot task per configured symbol.
pub fn spawn(state: &Arc<AppState>, config: BotConfig) {
    for symbol in state.symbols.clone() {
        let state = state.clone();
        tokio::spawn(async move { Bot::new(state, symbol, config).run().await });
    }
}

struct Bot {
    state: Arc<AppState>,
    symbol: String,
    config: BotConfig,
    rng: StdRng,
    reference: f64,
    fair: f64,
    /// Centre of the maker's ladder, in grid steps.
    centre: Option<f64>,
    epoch: u64,
    pre_market_since: Option<Instant>,
    seeded_auction: bool,
}

impl Bot {
    fn new(state: Arc<AppState>, symbol: String, config: BotConfig) -> Self {
        let reference = state.collar(&symbol).0.to_f64().unwrap_or(100.0);
        let last = state
            .market
            .lock()
            .unwrap()
            .symbols
            .get(&symbol)
            .and_then(|s| s.last)
            .and_then(|p| p.to_f64());
        let mut rng = StdRng::from_entropy();
        let fair = last.unwrap_or_else(|| reference * (1.0 + rng.gen_range(-0.015..0.015)));
        let epoch = state.reset_epoch.load(Ordering::SeqCst);
        Self {
            state,
            symbol,
            config,
            rng,
            reference,
            fair,
            centre: None,
            epoch,
            pre_market_since: None,
            seeded_auction: false,
        }
    }

    async fn run(mut self) {
        loop {
            let jitter = self.rng.gen_range(0.7..1.3);
            tokio::time::sleep(self.config.interval.mul_f64(jitter)).await;
            if let Err(e) = self.tick().await {
                tracing::debug!("{} bot: {e}", self.symbol);
            }
        }
    }

    async fn tick(&mut self) -> Result<(), String> {
        let epoch = self.state.reset_epoch.load(Ordering::SeqCst);
        if epoch != self.epoch {
            self.epoch = epoch;
            self.fair = self.reference * (1.0 + self.rng.gen_range(-0.015..0.015));
            self.centre = None;
            self.pre_market_since = None;
            self.seeded_auction = false;
        }
        let session = self
            .state
            .session(&self.symbol)
            .map_err(|e| e.to_string())?;
        let state = session.lock().await.state.clone();
        match state {
            SessionState::Open => {
                self.pre_market_since = None;
                self.walk();
                self.quote().await;
                if self.rng.gen_bool(TAKE_PROBABILITY) {
                    self.take().await;
                }
            }
            SessionState::PreMarket => {
                let since = *self.pre_market_since.get_or_insert_with(Instant::now);
                self.quote().await;
                if !self.seeded_auction {
                    self.seed_auction().await;
                    self.seeded_auction = true;
                }
                if since.elapsed() >= self.config.pre_open {
                    self.open().await?;
                }
            }
            SessionState::Halted { .. } | SessionState::PostMarket | SessionState::Closed => {}
        }
        Ok(())
    }

    fn walk(&mut self) {
        let shock = self.rng.gen_range(-1.0..1.0) * VOLATILITY * self.fair * 3f64.sqrt();
        self.fair += shock + MEAN_REVERSION * (self.reference - self.fair);
        let band = self.reference * MAX_DRIFT;
        self.fair = self
            .fair
            .clamp(self.reference - band, self.reference + band);
    }

    /// Maker ladder: cancel resting maker orders that are off the grid, add missing levels.
    async fn quote(&mut self) {
        let target = self.fair / GRID;
        let centre = match self.centre {
            Some(c) if (target - c).abs() < RECENTRE_STEPS => c,
            _ => target.round(),
        };
        self.centre = Some(centre);
        let bids: Vec<Decimal> = (0..LEVELS)
            .map(|k| grid_price(centre - HALF_SPREAD_STEPS - k as f64))
            .collect();
        let asks: Vec<Decimal> = (0..LEVELS)
            .map(|k| grid_price(centre + HALF_SPREAD_STEPS + k as f64))
            .collect();

        let resting = self
            .state
            .open_orders_in(&self.symbol, Some(MAKER_ID))
            .await;
        let mut have_bids = Vec::new();
        let mut have_asks = Vec::new();
        for order in &resting {
            let Some(price) = limit_price(order) else {
                continue;
            };
            let (wanted, have) = match order.side {
                Side::Buy => (&bids, &mut have_bids),
                Side::Sell => (&asks, &mut have_asks),
            };
            if wanted.contains(&price) && !have.contains(&price) {
                have.push(price);
            } else {
                let _ = self
                    .state
                    .cancel(order.id, &self.symbol, order.side.clone())
                    .await;
            }
        }
        for (side, wanted, have) in [
            (Side::Buy, &bids, &have_bids),
            (Side::Sell, &asks, &have_asks),
        ] {
            for price in wanted.iter().filter(|p| !have.contains(p)) {
                let size = self.size(50, 300);
                self.send(Order::new_limit(
                    Symbol::new(&self.symbol),
                    side.clone(),
                    *price,
                    size,
                    TimeInForce::GoodTillCanceled,
                    MAKER_ID,
                ))
                .await;
            }
        }
    }

    /// IOC order through the best opposite price, leaning towards fair value.
    async fn take(&mut self) {
        let Some(book) = self.state.book(&self.symbol).await else {
            return;
        };
        let (Some(bid), Some(ask)) = (book.bids.first(), book.asks.first()) else {
            return;
        };
        let mid = ((bid.0 + ask.0) / Decimal::TWO)
            .to_f64()
            .unwrap_or(self.fair);
        let lean = ((self.fair - mid) / (GRID * 4.0)).clamp(-0.3, 0.3);
        let side = if self.rng.gen_bool(0.5 + lean) {
            Side::Buy
        } else {
            Side::Sell
        };
        let through = Decimal::new(10, 2);
        let price = match side {
            Side::Buy => ask.0 + through,
            Side::Sell => bid.0 - through,
        };
        let size = self.size(10, 150);
        self.send(Order::new_limit(
            Symbol::new(&self.symbol),
            side,
            price,
            size,
            TimeInForce::ImmediateOrCancel,
            TAKER_ID,
        ))
        .await;
    }

    /// Pre-market: a buy above fair and a sell below it, so the opening auction has
    /// crossed orders to match.
    async fn seed_auction(&mut self) {
        for (side, steps) in [(Side::Buy, 2.0), (Side::Sell, -2.0)] {
            let price = grid_price((self.fair / GRID).round() + steps);
            let size = self.size(100, 400);
            self.send(Order::new_limit(
                Symbol::new(&self.symbol),
                side,
                price,
                size,
                TimeInForce::GoodTillCanceled,
                TAKER_ID,
            ))
            .await;
        }
    }

    /// Opens the session (running the auction), then withdraws whatever the taker left
    /// resting from its pre-market orders.
    async fn open(&mut self) -> Result<(), String> {
        let (_, trades) = self
            .state
            .transition(&self.symbol, |s| s.open())
            .await
            .map_err(|e| e.to_string())?;
        self.state.persist_trades(&trades).await;
        tracing::info!(
            "{} opened by the bot: {} auction trades",
            self.symbol,
            trades.len()
        );
        for order in self
            .state
            .open_orders_in(&self.symbol, Some(TAKER_ID))
            .await
        {
            let _ = self.state.cancel(order.id, &self.symbol, order.side).await;
        }
        Ok(())
    }

    async fn send(&self, order: Order) {
        match self.state.submit(order).await {
            Ok(trades) => self.state.persist_trades(&trades).await,
            Err(e) => tracing::debug!("{} bot order rejected: {e}", self.symbol),
        }
    }

    /// A random lot size between `min` and `max`, in tens.
    fn size(&mut self, min: u32, max: u32) -> Decimal {
        Decimal::from(self.rng.gen_range(min / 10..=max / 10) * 10)
    }
}

/// A grid price with two decimal places.
fn grid_price(steps: f64) -> Decimal {
    Decimal::new((steps * GRID * 100.0).round() as i64, 2)
}

fn limit_price(order: &Order) -> Option<Decimal> {
    match order.order_type {
        OrderType::Limit { price } => Some(price),
        _ => None,
    }
}
