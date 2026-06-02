use dashmap::DashMap;
use engine::session::TradingSession;
use engine::shard::EngineCommand;
use market_data::EventBus;
use risk::RiskEngine;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

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
