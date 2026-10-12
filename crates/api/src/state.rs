use common::Symbol;
use dashmap::DashMap;
use engine::session::TradingSession;
use engine::shard::{EngineCommand, start_shard};
use market_data::EventBus;
use persistence::wal::WriteAheadLog;
use risk::RiskEngine;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct AppState {
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
            shards,
            sessions,
            risk: Mutex::new(risk),
            bus,
            pool,
            wal: Mutex::new(None),
        }
    }
}
