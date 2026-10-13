use crate::market::MarketStats;
use crate::metrics::Metrics;
use crate::sandbox::{SandboxConfig, SandboxInfo};
use chrono::{DateTime, Utc};
use common::Symbol;
use dashmap::DashMap;
use engine::session::TradingSession;
use engine::shard::{EngineCommand, start_shard};
use market_data::EventBus;
use persistence::wal::WriteAheadLog;
use risk::RiskEngine;
use std::collections::HashSet;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantKind {
    Guest,
    Bot,
}

/// A named participant: a guest registered at runtime, or one of the in-process bots.
#[derive(Debug, Clone)]
pub struct ParticipantInfo {
    pub display_name: String,
    pub kind: ParticipantKind,
    /// When a guest joined. `None` for bots and for guests logged before this was recorded.
    pub registered_at: Option<DateTime<Utc>>,
}

pub struct AppState {
    /// Configured (shared) symbols, in `SYMBOLS` order. Sandboxes are not in here.
    pub symbols: Vec<String>,
    /// Sandbox symbol → its owner and activity. Their shards and sessions live in `shards`
    /// and `sessions` next to the shared symbols'.
    pub sandboxes: DashMap<String, SandboxInfo>,
    pub sandbox_config: SandboxConfig,
    /// symbol string → shard command channel
    pub shards: DashMap<String, mpsc::Sender<EngineCommand>>,
    /// symbol string → session state. Every mutating request on a symbol holds this lock
    /// from its checks through the shard reply, so session checks, WAL order and shard order agree.
    pub sessions: DashMap<String, Arc<tokio::sync::Mutex<TradingSession>>>,
    /// Behind a Mutex because fills update participant positions.
    pub risk: Mutex<RiskEngine>,
    pub bus: Arc<EventBus>,
    /// Optional Postgres pool — None when DATABASE_URL is not set
    pub pool: Option<sqlx::PgPool>,
    /// None while replaying at startup, so replayed entries aren't written twice.
    pub wal: Mutex<Option<WriteAheadLog>>,
    /// client_id → named participant (guests and bots). Guests are rebuilt from
    /// `ParticipantRegistered` WAL entries on replay.
    pub participants: DashMap<String, ParticipantInfo>,
    /// Per-symbol prices and volume, cash per participant, recent trades.
    pub market: Mutex<MarketStats>,
    /// Symbols whose book changed since the last `book_update` was published.
    pub dirty_books: Mutex<HashSet<String>>,
    /// Incremented by every market reset, so long-running tasks (bots) notice one.
    pub reset_epoch: AtomicU64,
    pub metrics: Metrics,
    /// Guest → when their last load burst started.
    pub burst_cooldowns: DashMap<String, std::time::Instant>,
}

impl AppState {
    /// Starts one shard and one PreMarket session per symbol. Must run inside a Tokio runtime.
    pub fn new(
        symbols: &[String],
        risk: RiskEngine,
        bus: Arc<EventBus>,
        pool: Option<sqlx::PgPool>,
    ) -> Self {
        let shards = DashMap::new();
        let sessions = DashMap::new();
        for sym in symbols {
            shards.insert(sym.clone(), start_shard(Symbol::new(sym.clone())));
            sessions.insert(
                sym.clone(),
                Arc::new(tokio::sync::Mutex::new(TradingSession::new())),
            );
        }
        Self {
            symbols: symbols.to_vec(),
            sandboxes: DashMap::new(),
            sandbox_config: SandboxConfig::default(),
            shards,
            sessions,
            risk: Mutex::new(risk),
            bus,
            pool,
            wal: Mutex::new(None),
            participants: DashMap::new(),
            market: Mutex::new(MarketStats::default()),
            dirty_books: Mutex::new(HashSet::new()),
            reset_epoch: AtomicU64::new(0),
            metrics: Metrics::default(),
            burst_cooldowns: DashMap::new(),
        }
    }
}
