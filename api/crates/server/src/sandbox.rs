//! Sandboxes: a private symbol per guest, which only its owner can see, trade and run.
//!
//! A sandbox starts in pre-market with the shared symbols' collar. Its owner opens, halts
//! and resumes it through the public listener, so they can rest crossed orders before the
//! open and watch the opening auction clear them, or halt it and watch orders get 409.
//! Creation and removal go to the WAL (`SymbolCreated`, `SymbolRemoved`), so sandboxes
//! survive a restart. Idle sandboxes are removed, the total is capped, and a market reset
//! removes them all.
//!
//! Sandbox trades change positions but not cash, so they never count towards P&L or the
//! leaderboard.

use crate::bots::MAKER_ID;
use crate::exchange::ExchangeError;
use crate::state::AppState;
use chrono::{DateTime, Utc};
use common::Symbol;
use engine::order::{Order, Side, TimeInForce};
use engine::session::{SessionState, TradingSession};
use engine::shard::start_shard;
use market_data::MarketEvent;
use persistence::wal::WalEntry;
use rand::Rng;
use risk::SymbolLimits;
use rust_decimal::Decimal;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use utoipa::ToSchema;

pub const SANDBOX_PREFIX: &str = "SBX-";
const NAME_LEN: usize = 6;
const NAME_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// Most resting seed orders a sandbox can hold; seeding again past this is refused.
const MAX_SEED_ORDERS: usize = 24;

#[derive(Debug, Clone, Copy)]
pub struct SandboxConfig {
    /// Most sandboxes that can exist at once.
    pub max: usize,
    /// Removed after this long without an order, cancel or session change.
    pub idle: Duration,
    pub reference_price: Decimal,
    pub price_collar_pct: Decimal,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max: 200,
            idle: Duration::from_secs(30 * 60),
            reference_price: Decimal::ONE_HUNDRED,
            price_collar_pct: Decimal::from(20u32),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SandboxInfo {
    pub owner: String,
    pub created_at: DateTime<Utc>,
    /// Last order, cancel or session change. Starts at creation (or at replay).
    pub last_active: Instant,
}

/// Who is asking, for deciding which symbols they can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer<'a> {
    /// The admin listener sees everything.
    Admin,
    /// The public listener, with the session's guest if there is one.
    Public(Option<&'a str>),
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SandboxResponse {
    pub symbol: String,
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
    /// Seconds of inactivity left before the sandbox is removed.
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct SeedResponse {
    /// Orders placed by the maker bot
    pub orders: usize,
    /// True when they were placed before the open, crossed, for the opening auction
    pub crossed: bool,
}

impl AppState {
    pub fn is_sandbox(&self, symbol: &str) -> bool {
        self.sandboxes.contains_key(symbol)
    }

    pub fn sandbox_owner(&self, symbol: &str) -> Option<String> {
        self.sandboxes.get(symbol).map(|s| s.owner.clone())
    }

    /// The sandbox `client_id` owns, if any.
    pub fn sandbox_of(&self, client_id: &str) -> Option<String> {
        self.sandboxes
            .iter()
            .find(|s| s.owner == client_id)
            .map(|s| s.key().clone())
    }

    /// Shared symbols are visible to everyone; a sandbox only to its owner and the admin.
    /// Symbols that don't exist are "visible", so callers report them as unknown.
    pub fn visible_to(&self, symbol: &str, viewer: Viewer) -> bool {
        match (self.sandbox_owner(symbol), viewer) {
            (None, _) | (_, Viewer::Admin) => true,
            (Some(owner), Viewer::Public(me)) => me == Some(owner.as_str()),
        }
    }

    pub(crate) fn touch_sandbox(&self, symbol: &str) {
        if let Some(mut sandbox) = self.sandboxes.get_mut(symbol) {
            sandbox.last_active = Instant::now();
        }
    }

    /// The owner's sandbox, created if they have none. Holds the WAL lock throughout, so
    /// the cap and the one-per-guest rule hold under concurrent requests.
    pub fn create_sandbox(&self, owner: &str) -> Result<String, ExchangeError> {
        let mut wal = self.wal.lock().unwrap();
        if let Some(existing) = self.sandbox_of(owner) {
            return Ok(existing);
        }
        if self.sandboxes.len() >= self.sandbox_config.max {
            return Err(ExchangeError::SandboxesFull(self.sandbox_config.max));
        }
        let symbol = loop {
            let name = new_sandbox_name();
            if !self.shards.contains_key(&name) {
                break name;
            }
        };
        let created_at = Utc::now();
        let config = self.sandbox_config;
        if let Some(wal) = wal.as_mut() {
            wal.append(&WalEntry::SymbolCreated {
                symbol: symbol.clone(),
                owner: owner.to_string(),
                reference_price: config.reference_price,
                price_collar_pct: config.price_collar_pct,
                created_at,
            })
            .map_err(ExchangeError::Wal)?;
        }
        self.apply_sandbox_created(
            &symbol,
            owner,
            config.reference_price,
            config.price_collar_pct,
            created_at,
        );
        Ok(symbol)
    }

    /// Removes a sandbox: closes its session so waiting orders get 409, logs the removal,
    /// drops its book, and tells the owner. Returns false if it didn't exist.
    pub async fn remove_sandbox(&self, symbol: &str, reason: &str) -> Result<bool, ExchangeError> {
        let Some(owner) = self.sandbox_owner(symbol) else {
            return Ok(false);
        };
        let session = self.session(symbol)?;
        let mut session = session.lock().await;
        if !self.is_sandbox(symbol) {
            return Ok(false); // removed while we waited for the lock
        }
        self.log(&WalEntry::SymbolRemoved {
            symbol: symbol.to_string(),
        })?;
        session.close();
        self.apply_sandbox_removed(symbol);
        self.bus.publish(MarketEvent::SandboxRemoved {
            symbol: Symbol::new(symbol),
            client_id: owner,
            reason: reason.to_string(),
        });
        Ok(true)
    }

    /// Starts the sandbox's shard and pre-market session and gives it a price collar.
    /// Live and replay.
    pub(crate) fn apply_sandbox_created(
        &self,
        symbol: &str,
        owner: &str,
        reference_price: Decimal,
        price_collar_pct: Decimal,
        created_at: DateTime<Utc>,
    ) {
        self.shards
            .insert(symbol.to_string(), start_shard(Symbol::new(symbol)));
        self.sessions.insert(
            symbol.to_string(),
            Arc::new(tokio::sync::Mutex::new(TradingSession::new())),
        );
        self.risk.lock().unwrap().symbol_limits.insert(
            Symbol::new(symbol),
            SymbolLimits {
                active: true,
                price_collar_pct,
                reference_price,
            },
        );
        self.sandboxes.insert(
            symbol.to_string(),
            SandboxInfo {
                owner: owner.to_string(),
                created_at,
                last_active: Instant::now(),
            },
        );
    }

    /// Forgets the sandbox: its shard stops once the last command channel is dropped, and
    /// its positions, stats and history go. Live and replay.
    pub(crate) fn apply_sandbox_removed(&self, symbol: &str) {
        self.sandboxes.remove(symbol);
        self.shards.remove(symbol);
        self.sessions.remove(symbol);
        {
            let mut risk = self.risk.lock().unwrap();
            let key = Symbol::new(symbol);
            risk.symbol_limits.remove(&key);
            for limits in risk.participant_limits.values_mut() {
                limits.current_position.remove(&key);
            }
        }
        {
            let mut market = self.market.lock().unwrap();
            market.symbols.remove(symbol);
            market.history.remove(symbol);
        }
        self.dirty_books.lock().unwrap().remove(symbol);
    }

    pub async fn sandbox_view(&self, symbol: &str) -> Option<SandboxResponse> {
        let info = self.sandboxes.get(symbol)?.clone();
        let session = self.session(symbol).ok()?;
        let state = session.lock().await.state.clone();
        let idle_for = info.last_active.elapsed();
        Some(SandboxResponse {
            symbol: symbol.to_string(),
            state,
            created_at: info.created_at,
            expires_in_secs: self.sandbox_config.idle.saturating_sub(idle_for).as_secs(),
        })
    }

    /// Has the maker bot place sample orders around the sandbox's last price. Before the
    /// open they cross, so opening runs an auction; once open they form a two-sided book.
    pub async fn seed_sandbox(&self, symbol: &str) -> Result<SeedResponse, ExchangeError> {
        if !self.is_sandbox(symbol) {
            return Err(ExchangeError::UnknownSymbol(symbol.to_string()));
        }
        let resting = self.open_orders_in(symbol, Some(MAKER_ID)).await.len();
        if resting >= MAX_SEED_ORDERS {
            return Err(ExchangeError::SandboxConflict(format!(
                "{symbol} already has {resting} sample orders resting; cancel or trade some first"
            )));
        }
        let state = self.session(symbol)?.lock().await.state.clone();
        let mark = self
            .market
            .lock()
            .unwrap()
            .symbols
            .get(symbol)
            .and_then(|s| s.last)
            .unwrap_or(self.sandbox_config.reference_price);
        let cents = |n: i64| mark + Decimal::new(n, 2);
        let crossed = state == SessionState::PreMarket;
        // (side, price offset in cents, size)
        let ladder: &[(Side, i64, u32)] = if crossed {
            &[
                (Side::Buy, 20, 100),
                (Side::Buy, 10, 150),
                (Side::Buy, 0, 200),
                (Side::Sell, -10, 120),
                (Side::Sell, 0, 150),
                (Side::Sell, 10, 180),
            ]
        } else {
            &[
                (Side::Buy, -5, 150),
                (Side::Buy, -10, 200),
                (Side::Buy, -15, 250),
                (Side::Sell, 5, 150),
                (Side::Sell, 10, 200),
                (Side::Sell, 15, 250),
            ]
        };
        let mut placed = 0;
        for (side, offset, size) in ladder {
            let order = Order::new_limit(
                Symbol::new(symbol),
                side.clone(),
                cents(*offset),
                Decimal::from(*size),
                TimeInForce::GoodTillCanceled,
                MAKER_ID,
            );
            let trades = self.submit(order).await?;
            self.persist_trades(&trades).await;
            placed += 1;
        }
        Ok(SeedResponse {
            orders: placed,
            crossed,
        })
    }

    /// Removes sandboxes idle for longer than the configured limit, checking every `every`.
    pub fn spawn_sandbox_sweeper(self: &Arc<Self>, every: Duration) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(every);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                state.sweep_idle_sandboxes().await;
            }
        });
    }

    /// Removes every sandbox idle for longer than the configured limit. Returns how many.
    pub async fn sweep_idle_sandboxes(&self) -> usize {
        let idle = self.sandbox_config.idle;
        let expired: Vec<String> = self
            .sandboxes
            .iter()
            .filter(|s| s.last_active.elapsed() >= idle)
            .map(|s| s.key().clone())
            .collect();
        let mut removed = 0;
        for symbol in expired {
            match self.remove_sandbox(&symbol, "idle").await {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(e) => tracing::warn!("could not remove idle sandbox {symbol}: {e}"),
            }
        }
        if removed > 0 {
            tracing::info!("removed {removed} idle sandboxes");
        }
        removed
    }
}

/// `SBX-` and six characters that can't be misread (no 0/O or 1/I).
fn new_sandbox_name() -> String {
    let mut rng = rand::thread_rng();
    let suffix: String = (0..NAME_LEN)
        .map(|_| NAME_ALPHABET[rng.gen_range(0..NAME_ALPHABET.len())] as char)
        .collect();
    format!("{SANDBOX_PREFIX}{suffix}")
}
