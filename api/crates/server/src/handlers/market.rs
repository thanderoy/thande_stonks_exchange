//! Market-wide reads: symbols, recent trades and the leaderboard.

use crate::handlers::guest;
use crate::handlers::orders::ErrorResponse;
use crate::market::{HISTORY_LIMIT, LeaderboardEntry};
use crate::sandbox::Viewer;
use crate::state::AppState;
use actix_session::Session;
use actix_web::{HttpResponse, web};
use engine::matcher::Trade;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

const DEFAULT_TRADES: usize = 200;
const DEFAULT_LEADERS: usize = 10;
const MAX_LEADERS: usize = 50;

#[derive(Deserialize, IntoParams)]
pub struct LimitQuery {
    /// Maximum number of rows to return
    pub limit: Option<usize>,
}

#[derive(Serialize, ToSchema)]
pub struct TradesResponse {
    pub symbol: String,
    /// Newest first
    pub trades: Vec<Trade>,
}

#[derive(Serialize, ToSchema)]
pub struct LeaderboardResponse {
    /// Best P&L first
    pub entries: Vec<LeaderboardEntry>,
}

#[utoipa::path(
    get,
    path = "/v1/symbols",
    responses(
        (status = 200, description = "Every symbol with its session state, price stats since the last reset and price collar: the shared symbols, then sandboxes (all of them here; on the public listener only the caller's own)", body = [SymbolSummary]),
    ),
    tag = "market"
)]
pub async fn symbols(data: web::Data<Arc<AppState>>) -> HttpResponse {
    HttpResponse::Ok().json(data.symbol_summaries(Viewer::Admin).await)
}

/// Public listener: the shared symbols plus the caller's own sandbox.
pub async fn symbols_public(session: Session, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let me = guest::current_participant(&session, &data);
    let viewer = Viewer::Public(me.as_ref().map(|p| p.client_id.as_str()));
    HttpResponse::Ok().json(data.symbol_summaries(viewer).await)
}

/// Public listener: a sandbox's trades are visible only to its owner.
pub async fn trades_public(
    path: web::Path<String>,
    query: web::Query<LimitQuery>,
    session: Session,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    if !guest::can_see(&session, &data, &path) {
        return guest::symbol_not_found(&path);
    }
    trades(path, query, data).await
}

#[utoipa::path(
    get,
    path = "/v1/trades/{symbol}",
    params(("symbol" = String, Path, description = "Ticker symbol"), LimitQuery),
    responses(
        (status = 200, description = "Recent trades since the last reset or restart, newest first (at most 500; default 200)", body = TradesResponse),
        (status = 404, description = "Symbol not registered", body = ErrorResponse),
    ),
    tag = "market"
)]
pub async fn trades(
    path: web::Path<String>,
    query: web::Query<LimitQuery>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let symbol = path.into_inner();
    let limit = query.limit.unwrap_or(DEFAULT_TRADES).min(HISTORY_LIMIT);
    match data.recent_trades(&symbol, limit) {
        Some(trades) => HttpResponse::Ok().json(TradesResponse { symbol, trades }),
        None => HttpResponse::NotFound().json(ErrorResponse {
            error: "symbol not found".into(),
        }),
    }
}

#[utoipa::path(
    get,
    path = "/v1/leaderboard",
    params(LimitQuery),
    responses(
        (status = 200, description = "Guests and bots who have traded since the last reset, by P&L (cash plus positions at the last price). On the public listener `you` marks the caller.", body = LeaderboardResponse),
    ),
    tag = "market"
)]
pub async fn leaderboard(
    query: web::Query<LimitQuery>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    leaderboard_for(&query, None, &data)
}

/// Public listener: also marks the session's guest.
pub async fn leaderboard_public(
    query: web::Query<LimitQuery>,
    session: Session,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let me = guest::current_participant(&session, &data);
    leaderboard_for(&query, me.as_ref().map(|p| p.client_id.as_str()), &data)
}

fn leaderboard_for(query: &LimitQuery, viewer: Option<&str>, data: &AppState) -> HttpResponse {
    let limit = query.limit.unwrap_or(DEFAULT_LEADERS).min(MAX_LEADERS);
    HttpResponse::Ok().json(LeaderboardResponse {
        entries: data.leaderboard(limit, viewer),
    })
}
