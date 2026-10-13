//! The engine view (`GET /v1/engine/{symbol}`), load stats and bursts, and `/metrics`.

use crate::burst::{ADMIN_MAX_BURST, GUEST_MAX_BURST};
use crate::exchange::ExchangeError;
use crate::handlers::guest;
use crate::handlers::orders::ErrorResponse;
use crate::state::AppState;
use actix_session::Session;
use actix_web::{HttpResponse, web};
use chrono::{DateTime, Utc};
use common::OrderId;
use engine::session::SessionState;
use engine::shard::{EngineCommand, QueueLevel, QueueSnapshot};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use utoipa::{IntoParams, ToSchema};

const DEFAULT_LEVELS: usize = 6;
const MAX_LEVELS: usize = 20;
const ORDERS_PER_LEVEL: usize = 40;

#[derive(Deserialize, IntoParams)]
pub struct EngineQuery {
    /// Price levels per side (default 6, at most 20)
    pub levels: Option<usize>,
}

#[derive(Serialize, ToSchema)]
pub struct QueueOrderView {
    pub order_id: OrderId,
    /// Display name, bot name or pre-registered id; never a guest's client id
    pub participant: String,
    pub remaining: Decimal,
    pub submitted_at: DateTime<Utc>,
    /// True for the caller's own orders, on the public listener
    pub you: bool,
}

#[derive(Serialize, ToSchema)]
pub struct QueueLevelView {
    pub price: Decimal,
    pub total: Decimal,
    /// Front of the queue (fills first) first, at most 40
    pub orders: Vec<QueueOrderView>,
    /// Orders further back that were left out
    pub hidden: usize,
}

#[derive(Serialize, ToSchema)]
pub struct EngineResponse {
    pub symbol: String,
    pub state: SessionState,
    /// Best (highest) bid first
    pub bids: Vec<QueueLevelView>,
    /// Best (lowest) ask first
    pub asks: Vec<QueueLevelView>,
}

#[derive(Deserialize, ToSchema)]
pub struct BurstRequest {
    /// Orders to fire: at most 200 on the public listener, 5000 on the admin listener
    pub count: usize,
}

#[derive(Deserialize, ToSchema)]
pub struct AdminBurstRequest {
    pub symbol: String,
    /// At most 5000
    pub count: usize,
}

fn failed(e: ExchangeError) -> HttpResponse {
    let mut resp = HttpResponse::build(e.status());
    if let ExchangeError::BurstCooldown(secs) = e {
        resp.insert_header(("Retry-After", secs.to_string()));
    }
    resp.json(ErrorResponse {
        error: e.to_string(),
    })
}

async fn queues(
    data: &AppState,
    symbol: &str,
    levels: usize,
    viewer: Option<&str>,
) -> Option<EngineResponse> {
    let shard = data.shards.get(symbol)?.clone();
    let state = data
        .sessions
        .get(symbol)?
        .clone()
        .lock()
        .await
        .state
        .clone();
    let (reply, mut rx) = mpsc::channel(1);
    shard
        .send(EngineCommand::Queues {
            levels: levels.clamp(1, MAX_LEVELS),
            orders_per_level: ORDERS_PER_LEVEL,
            reply,
        })
        .await
        .ok()?;
    let QueueSnapshot { bids, asks } = rx.recv().await?;
    let view = |level: QueueLevel| QueueLevelView {
        price: level.price,
        total: level.total,
        hidden: level.hidden,
        orders: level
            .orders
            .into_iter()
            .map(|o| QueueOrderView {
                order_id: o.id,
                participant: data.participant_label(&o.client_id),
                remaining: o.remaining,
                submitted_at: o.submitted_at,
                you: viewer == Some(o.client_id.as_str()),
            })
            .collect(),
    };
    Some(EngineResponse {
        symbol: symbol.to_string(),
        state,
        bids: bids.into_iter().map(view).collect(),
        asks: asks.into_iter().map(view).collect(),
    })
}

#[utoipa::path(
    get,
    path = "/v1/engine/{symbol}",
    params(("symbol" = String, Path, description = "Ticker symbol"), EngineQuery),
    responses(
        (status = 200, description = "The book as the engine holds it: each price level's orders in queue order, labelled by participant. On the public listener `you` marks the caller's orders, and a sandbox is visible only to its owner.", body = EngineResponse),
        (status = 404, description = "Symbol not found", body = ErrorResponse),
    ),
    tag = "engine"
)]
pub async fn engine(
    path: web::Path<String>,
    query: web::Query<EngineQuery>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let levels = query.levels.unwrap_or(DEFAULT_LEVELS);
    match queues(&data, &path, levels, None).await {
        Some(view) => HttpResponse::Ok().json(view),
        None => guest::symbol_not_found(&path),
    }
}

pub async fn engine_public(
    path: web::Path<String>,
    query: web::Query<EngineQuery>,
    session: Session,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    if !guest::can_see(&session, &data, &path) {
        return guest::symbol_not_found(&path);
    }
    let me = guest::current_participant(&session, &data);
    let levels = query.levels.unwrap_or(DEFAULT_LEVELS);
    match queues(
        &data,
        &path,
        levels,
        me.as_ref().map(|p| p.client_id.as_str()),
    )
    .await
    {
        Some(view) => HttpResponse::Ok().json(view),
        None => guest::symbol_not_found(&path),
    }
}

#[utoipa::path(
    get,
    path = "/v1/stats",
    responses(
        (status = 200, description = "Order, trade and latency counts: totals since start, and the last 60 seconds second by second", body = crate::metrics::StatsResponse),
    ),
    tag = "engine"
)]
pub async fn stats(data: web::Data<Arc<AppState>>) -> HttpResponse {
    HttpResponse::Ok().json(data.metrics.stats())
}

/// `GET /metrics` (admin listener): Prometheus text format.
pub async fn prometheus(data: web::Data<Arc<AppState>>) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4; charset=utf-8")
        .body(data.metrics.prometheus())
}

#[utoipa::path(
    post,
    path = "/v1/me/sandbox/burst",
    request_body = BurstRequest,
    responses(
        (status = 200, description = "Fired `count` random orders from the load bot at the caller's open sandbox, then cancelled what they left resting. Public listener only.", body = crate::burst::BurstResult),
        (status = 400, description = "count is 0 or over 200", body = ErrorResponse),
        (status = 401, description = "No valid guest session", body = ErrorResponse),
        (status = 404, description = "The caller has no sandbox", body = ErrorResponse),
        (status = 409, description = "The sandbox isn't open", body = ErrorResponse),
        (status = 429, description = "Less than 10 seconds since the caller's last burst", body = ErrorResponse),
    ),
    tag = "sandbox"
)]
pub async fn burst_sandbox(
    body: web::Json<BurstRequest>,
    session: Session,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let Some(me) = guest::current_participant(&session, &data) else {
        return guest::unauthorized();
    };
    if body.count == 0 || body.count > GUEST_MAX_BURST {
        return HttpResponse::BadRequest().json(ErrorResponse {
            error: format!("count must be 1-{GUEST_MAX_BURST}"),
        });
    }
    let Some(symbol) = data.sandbox_of(&me.client_id) else {
        return HttpResponse::NotFound().json(ErrorResponse {
            error: "you have no sandbox; POST /v1/me/sandbox to create one".into(),
        });
    };
    let wait = data.take_burst_slot(&me.client_id);
    if wait > 0 {
        return failed(ExchangeError::BurstCooldown(wait));
    }
    match data.burst(&symbol, body.count).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            // Nothing was fired, so it doesn't count towards the cooldown.
            data.burst_cooldowns.remove(&me.client_id);
            failed(e)
        }
    }
}

#[utoipa::path(
    post,
    path = "/v1/admin/burst",
    request_body = AdminBurstRequest,
    responses(
        (status = 200, description = "Fired `count` random orders from the load bot at an open symbol, then cancelled what they left resting", body = crate::burst::BurstResult),
        (status = 400, description = "count is 0 or over 5000", body = ErrorResponse),
        (status = 404, description = "Symbol not found", body = ErrorResponse),
        (status = 409, description = "The symbol isn't open", body = ErrorResponse),
    ),
    tag = "admin"
)]
pub async fn burst_admin(
    body: web::Json<AdminBurstRequest>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    if body.count == 0 || body.count > ADMIN_MAX_BURST {
        return HttpResponse::BadRequest().json(ErrorResponse {
            error: format!("count must be 1-{ADMIN_MAX_BURST}"),
        });
    }
    match data.burst(&body.symbol, body.count).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => failed(e),
    }
}
