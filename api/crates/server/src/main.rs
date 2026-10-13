use actix_web::cookie::Key;
use actix_web::{App, HttpServer, middleware, web};
use common::Symbol;
use market_data::EventBus;
use persistence::wal::WriteAheadLog;
use risk::{ParticipantLimits, RiskEngine, SymbolLimits};
use rust_decimal::Decimal;
use server::app::{self, PublicConfig, PublicLimits};
use server::client_ip::parse_trusted_proxies;
use server::state::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("server=info".parse().unwrap()),
        )
        .init();

    let bind_addr = std::env::var("API_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let admin_bind_addr =
        std::env::var("ADMIN_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:9090".to_string());
    let public_config = public_config_from_env()?;
    let reset_at = match std::env::var("RESET_AT") {
        Ok(raw) if !raw.trim().is_empty() => Some(
            server::reset::parse_reset_at(&raw)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
        ),
        _ => None,
    };
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
                // High enough that the monitor's order swarm isn't throttled.
                max_orders_per_second: 100_000,
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

    let risk = RiskEngine::new(participant_limits, symbol_limits);
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

    state.spawn_book_publisher(Duration::from_millis(100));
    match reset_at {
        Some(at) => {
            state.spawn_daily_reset(at, chrono::Duration::hours(app::SESSION_TTL_HOURS));
        }
        None => tracing::info!("No daily market reset (set RESET_AT=HH:MM, UTC)"),
    }

    tracing::info!("Public listener on http://{bind_addr}");
    tracing::info!("Admin listener on http://{admin_bind_addr} (Swagger UI at /swagger-ui/)");

    let public_state = state.clone();
    let public = HttpServer::new(move || {
        let config = public_config.clone();
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(public_state.clone()))
            .configure(move |cfg| app::configure_public(cfg, &config))
    })
    .bind(&bind_addr)?
    .run();

    let admin = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(state.clone()))
            .configure(app::configure_admin)
    })
    .bind(&admin_bind_addr)?
    .run();

    tokio::try_join!(public, admin)?;
    Ok(())
}

fn public_config_from_env() -> std::io::Result<PublicConfig> {
    let invalid = |msg: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, msg);
    let session_key = match std::env::var("SESSION_KEY") {
        Ok(raw) if !raw.trim().is_empty() => app::parse_session_key(&raw).map_err(invalid)?,
        _ => {
            tracing::warn!(
                "SESSION_KEY not set: using a random key, so guest sessions won't survive a restart"
            );
            Key::generate()
        }
    };
    let cookie_secure = app::parse_cookie_secure(std::env::var("COOKIE_SECURE").ok().as_deref());
    if !cookie_secure {
        tracing::warn!("COOKIE_SECURE=false: session cookies will be sent over plain HTTP");
    }
    let trusted_proxies =
        parse_trusted_proxies(&std::env::var("TRUSTED_PROXY_CIDRS").unwrap_or_default())
            .map_err(invalid)?;
    Ok(PublicConfig::new(
        session_key,
        cookie_secure,
        trusted_proxies,
        PublicLimits::default(),
    ))
}
