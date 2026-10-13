//! Load bursts: fire a batch of random orders at one symbol and report how the engine
//! coped. Guests can burst only their own open sandbox, at most `GUEST_MAX_BURST` orders
//! once per `GUEST_COOLDOWN`; the admin listener can burst any open symbol.
//!
//! The orders come from the load bot and go through `AppState::submit` like any other, so
//! they are risk checked, logged and matched for real. Whatever they leave resting is
//! cancelled at the end, so repeated bursts don't pile up on the book.

use crate::exchange::ExchangeError;
use crate::state::AppState;
use common::Symbol;
use engine::order::{Order, Side, TimeInForce};
use engine::session::SessionState;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rust_decimal::Decimal;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use utoipa::ToSchema;

pub const LOAD_ID: &str = "bot-load";
pub const GUEST_MAX_BURST: usize = 200;
pub const ADMIN_MAX_BURST: usize = 5_000;
pub const GUEST_COOLDOWN: Duration = Duration::from_secs(10);
/// Orders in flight at once.
const CONCURRENCY: usize = 8;

#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct BurstResult {
    pub symbol: String,
    pub submitted: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub trades: usize,
    pub elapsed_ms: u64,
    pub orders_per_sec: u64,
    /// Submit latency percentiles in microseconds
    pub p50_us: u32,
    pub p99_us: u32,
    /// Load-bot orders left resting and then cancelled
    pub cancelled: usize,
}

impl AppState {
    /// Seconds a guest must still wait before their next burst; 0 when they may go now.
    /// Starts their cooldown when it returns 0.
    pub fn take_burst_slot(&self, client_id: &str) -> u64 {
        let now = Instant::now();
        if let Some(last) = self.burst_cooldowns.get(client_id).map(|t| *t) {
            let since = now.saturating_duration_since(last);
            if since < GUEST_COOLDOWN {
                return (GUEST_COOLDOWN - since).as_secs().max(1);
            }
        }
        self.burst_cooldowns.insert(client_id.to_string(), now);
        0
    }

    /// Fires `count` random orders at an open symbol, `CONCURRENCY` at a time, then
    /// cancels what the load bot left resting.
    pub async fn burst(
        self: &Arc<Self>,
        symbol: &str,
        count: usize,
    ) -> Result<BurstResult, ExchangeError> {
        let state = self.session(symbol)?.lock().await.state.clone();
        if state != SessionState::Open {
            return Err(ExchangeError::NotAcceptingOrders(symbol.to_string(), state));
        }
        let mark = self
            .market
            .lock()
            .unwrap()
            .symbols
            .get(symbol)
            .and_then(|s| s.last)
            .unwrap_or_else(|| self.collar(symbol).0);

        let started = Instant::now();
        let mut latencies = Vec::with_capacity(count);
        let (mut accepted, mut trades) = (0, 0);
        let mut tasks = JoinSet::new();
        let mut rng = StdRng::from_entropy();
        let mut next = 0;
        while next < count || !tasks.is_empty() {
            while next < count && tasks.len() < CONCURRENCY {
                let order = random_order(&mut rng, symbol, mark);
                let state = self.clone();
                tasks.spawn(async move {
                    let t = Instant::now();
                    let result = state.submit(order).await;
                    if let Ok(fills) = &result {
                        state.persist_trades(fills).await;
                    }
                    (t.elapsed(), result.map(|f| f.len()))
                });
                next += 1;
            }
            if let Some(Ok((latency, result))) = tasks.join_next().await {
                latencies.push(latency.as_micros().min(u32::MAX as u128) as u32);
                if let Ok(fills) = result {
                    accepted += 1;
                    trades += fills;
                }
            }
        }
        let elapsed = started.elapsed();

        let mut cancelled = 0;
        for order in self.open_orders_in(symbol, Some(LOAD_ID)).await {
            if let Ok(Some(_)) = self.cancel(order.id, symbol, order.side).await {
                cancelled += 1;
            }
        }

        latencies.sort_unstable();
        let pct = |q: f64| -> u32 {
            if latencies.is_empty() {
                return 0;
            }
            let rank = ((q * latencies.len() as f64).ceil() as usize).clamp(1, latencies.len());
            latencies[rank - 1]
        };
        Ok(BurstResult {
            symbol: symbol.to_string(),
            submitted: count,
            accepted,
            rejected: count - accepted,
            trades,
            elapsed_ms: elapsed.as_millis() as u64,
            orders_per_sec: (count as f64 / elapsed.as_secs_f64().max(0.001)) as u64,
            p50_us: pct(0.50),
            p99_us: pct(0.99),
            cancelled,
        })
    }
}

/// A limit order within 50 cents of `mark`, either side; 60% IOC, the rest GTC.
fn random_order(rng: &mut StdRng, symbol: &str, mark: Decimal) -> Order {
    let side = if rng.gen_bool(0.5) {
        Side::Buy
    } else {
        Side::Sell
    };
    let offset = Decimal::new(rng.gen_range(-50..=50), 2);
    let tif = if rng.gen_bool(0.6) {
        TimeInForce::ImmediateOrCancel
    } else {
        TimeInForce::GoodTillCanceled
    };
    Order::new_limit(
        Symbol::new(symbol),
        side,
        (mark + offset).round_dp(2),
        Decimal::from(rng.gen_range(1..=10) * 10),
        tif,
        LOAD_ID,
    )
}
