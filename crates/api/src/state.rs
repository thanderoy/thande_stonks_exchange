use std::sync::{Arc, Mutex};
use dashmap::DashMap;
use tokio::sync::mpsc;
use engine::shard::EngineCommand;
use engine::session::TradingSession;
use risk::RiskEngine;
use market_data::EventBus;

pub struct AppState {
    /// symbol string → shard command channel
    pub shards: DashMap<String, mpsc::Sender<EngineCommand>>,
    /// symbol string → session state (Mutex for interior mutability across shared ref)
    pub sessions: DashMap<String, Arc<Mutex<TradingSession>>>,
    pub risk: RiskEngine,
    pub bus: Arc<EventBus>,
    /// Optional Postgres pool — None when DATABASE_URL is not set
    pub pool: Option<sqlx::PgPool>,
}
