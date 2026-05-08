use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use actix_web::{middleware, web, App, HttpServer};
use dashmap::DashMap;
use engine::session::TradingSession;
use engine::shard::start_shard;
use market_data::EventBus;
use risk::{RiskEngine, ParticipantLimits, SymbolLimits};
use common::Symbol;
use rust_decimal::Decimal;
use api::handlers::{health, orders, session, ws};
use api::state::AppState;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("api=info".parse().unwrap()))
        .init();

    let bind_addr = std::env::var("API_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let symbols_env = std::env::var("SYMBOLS").unwrap_or_else(|_| "AAPL".to_string());
    let symbols: Vec<String> = symbols_env.split(',').map(|s| s.trim().to_string()).collect();

    let pool = match std::env::var("DATABASE_URL") {
        Ok(url) => {
            tracing::info!("Connecting to Postgres...");
            match persistence::ledger::connect(&url).await {
                Ok(p) => {
                    tracing::info!("Postgres connected");
                    Some(p)
                }
                Err(e) => {
                    tracing::warn!("Postgres connect failed: {e} — running without DB persistence");
                    None
                }
            }
        }
        Err(_) => {
            tracing::info!("DATABASE_URL not set — running without DB persistence");
            None
        }
    };

    let shards = DashMap::new();
    let sessions: DashMap<String, Arc<Mutex<TradingSession>>> = DashMap::new();

    for sym in &symbols {
        let symbol = Symbol::new(sym.clone());
        let tx = start_shard(symbol);
        shards.insert(sym.clone(), tx);
        sessions.insert(sym.clone(), Arc::new(Mutex::new(TradingSession::new())));
        tracing::info!("Shard started for {sym}");
    }

    // Build a permissive RiskEngine with a "demo" participant pre-registered.
    // Real deployments would load participant limits from a config file or DB.
    let mut participant_limits = HashMap::new();
    for name in ["demo", "trader1", "trader2"] {
        participant_limits.insert(name.to_string(), ParticipantLimits {
            max_notional_per_order: Decimal::from(10_000_000u64),
            max_orders_per_second: 1000,
            current_position: HashMap::new(),
            max_position: Decimal::from(1_000_000u64),
        });
    }

    let mut symbol_limits = HashMap::new();
    for sym in &symbols {
        symbol_limits.insert(Symbol::new(sym.clone()), SymbolLimits {
            active: true,
            price_collar_pct: Decimal::from(20u32),
            reference_price: Decimal::from(100u32),
        });
    }

    let risk = RiskEngine { participant_limits, symbol_limits };
    let bus = Arc::new(EventBus::new(1024));

    let state = Arc::new(AppState { shards, sessions, risk, bus, pool });

    tracing::info!("Listening on http://{bind_addr}");

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(state.clone()))
            .route("/healthz", web::get().to(health::healthz))
            .route("/v1/orders", web::post().to(orders::submit_order))
            .route("/v1/orders/{id}", web::delete().to(orders::cancel_order))
            .route("/v1/session/{symbol}", web::get().to(session::get_session))
            .route("/v1/session/{symbol}/open", web::post().to(session::open_session))
            .route("/v1/session/{symbol}/halt", web::post().to(session::halt_session))
            .route("/v1/session/{symbol}/resume", web::post().to(session::resume_session))
            .route("/v1/stream", web::get().to(ws::stream))
    })
    .bind(&bind_addr)?
    .run()
    .await
}
