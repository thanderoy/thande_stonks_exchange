use actix_web::{App, HttpServer, middleware, web};
use api::handlers::{book, health, orders, session, ws};
use api::openapi::ApiDoc;
use api::state::AppState;
use common::Symbol;
use market_data::EventBus;
use persistence::wal::WriteAheadLog;
use risk::{ParticipantLimits, RiskEngine, SymbolLimits};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("api=info".parse().unwrap()),
        )
        .init();

    let bind_addr = std::env::var("API_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let symbols_env = std::env::var("SYMBOLS").unwrap_or_else(|_| "AAPL".to_string());
    let symbols: Vec<String> = symbols_env
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

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

    let mut participant_limits = HashMap::new();
    for name in ["demo", "trader1", "trader2"] {
        participant_limits.insert(
            name.to_string(),
            ParticipantLimits {
                max_notional_per_order: Decimal::from(10_000_000u64),
                max_orders_per_second: 1000,
                current_position: HashMap::new(),
                max_position: Decimal::from(1_000_000u64),
            },
        );
    }

    let mut symbol_limits = HashMap::new();
    for sym in &symbols {
        symbol_limits.insert(
            Symbol::new(sym.clone()),
            SymbolLimits {
                active: true,
                price_collar_pct: Decimal::from(20u32),
                reference_price: Decimal::from(100u32),
            },
        );
    }

    let risk = RiskEngine {
        participant_limits,
        symbol_limits,
    };
    let bus = Arc::new(EventBus::new(1024));
    let state = Arc::new(AppState::new(&symbols, risk, bus, pool));

    let wal_path = std::env::var("WAL_PATH").unwrap_or_else(|_| "exchange.wal".to_string());
    let (wal, entries) = WriteAheadLog::open_for_recovery(&wal_path)?;
    let stats = state
        .replay(entries)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    tracing::info!(
        "Replayed {} WAL entries from {wal_path} ({} skipped for unconfigured symbols)",
        stats.applied,
        stats.skipped
    );
    *state.wal.lock().unwrap() = Some(wal);

    let openapi = ApiDoc::openapi();

    tracing::info!("Listening on http://{bind_addr}");
    tracing::info!("Swagger UI: http://{bind_addr}/swagger-ui/");

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(state.clone()))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}").url("/api-docs/openapi.json", openapi.clone()),
            )
            .route("/healthz", web::get().to(health::healthz))
            .route("/v1/orders", web::post().to(orders::submit_order))
            .route("/v1/orders/{id}", web::delete().to(orders::cancel_order))
            .route("/v1/session/{symbol}", web::get().to(session::get_session))
            .route(
                "/v1/session/{symbol}/open",
                web::post().to(session::open_session),
            )
            .route(
                "/v1/session/{symbol}/halt",
                web::post().to(session::halt_session),
            )
            .route(
                "/v1/session/{symbol}/resume",
                web::post().to(session::resume_session),
            )
            .route("/v1/book/{symbol}", web::get().to(book::get_book))
            .route("/v1/stream", web::get().to(ws::stream))
    })
    .bind(&bind_addr)?
    .run()
    .await
}
