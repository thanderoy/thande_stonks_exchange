use crate::state::AppState;
use actix_web::{HttpResponse, web};
use engine::session::{HaltReason, TradingSession};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct SessionStateResponse {
    pub symbol: String,
    pub state: engine::session::SessionState,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Deserialize, ToSchema)]
pub struct HaltBody {
    /// One of "circuit_breaker", "regulatory", or any other value for TechnicalIssue
    #[serde(default)]
    pub reason: Option<String>,
    /// Required when reason is "circuit_breaker" (decimal string, e.g. "10")
    pub threshold_pct: Option<Decimal>,
}

#[utoipa::path(
    get,
    path = "/v1/session/{symbol}",
    params(("symbol" = String, Path, description = "Ticker symbol")),
    responses(
        (status = 200, description = "Current session state", body = SessionStateResponse),
        (status = 404, description = "Symbol not found", body = ErrorResponse),
    ),
    tag = "session"
)]
pub async fn get_session(path: web::Path<String>, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let symbol = path.into_inner();
    match data.sessions.get(&symbol) {
        Some(session) => {
            let state = session.lock().await.state.clone();
            HttpResponse::Ok().json(SessionStateResponse { symbol, state })
        }
        None => HttpResponse::NotFound().json(ErrorResponse {
            error: "symbol not found".into(),
        }),
    }
}

#[utoipa::path(
    post,
    path = "/v1/session/{symbol}/open",
    params(("symbol" = String, Path, description = "Ticker symbol")),
    responses(
        (status = 200, description = "Session opened; resting pre-open orders are uncrossed in an opening auction", body = SessionStateResponse),
        (status = 404, description = "Symbol not found", body = ErrorResponse),
    ),
    tag = "session"
)]
pub async fn open_session(path: web::Path<String>, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let symbol = path.into_inner();
    transition_response(&data, symbol, |s| s.open()).await
}

/// Applies a transition, then emits the session event and any opening-auction trades.
async fn transition_response(
    data: &AppState,
    symbol: String,
    change: impl FnOnce(&mut TradingSession),
) -> HttpResponse {
    match data.transition(&symbol, change).await {
        Ok((state, trades)) => {
            data.publish_session(&symbol, &state);
            data.emit_trades(&trades).await;
            HttpResponse::Ok().json(SessionStateResponse { symbol, state })
        }
        Err(e) => HttpResponse::build(e.status()).json(ErrorResponse {
            error: e.to_string(),
        }),
    }
}

#[utoipa::path(
    post,
    path = "/v1/session/{symbol}/halt",
    params(("symbol" = String, Path, description = "Ticker symbol")),
    request_body = HaltBody,
    responses(
        (status = 200, description = "Session halted", body = SessionStateResponse),
        (status = 404, description = "Symbol not found", body = ErrorResponse),
    ),
    tag = "session"
)]
pub async fn halt_session(
    path: web::Path<String>,
    body: web::Json<HaltBody>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let symbol = path.into_inner();
    let reason = match body.reason.as_deref() {
        Some("circuit_breaker") => HaltReason::CircuitBreaker {
            threshold_pct: body.threshold_pct.unwrap_or(Decimal::TEN),
        },
        Some("regulatory") => HaltReason::RegulatoryHalt,
        _ => HaltReason::TechnicalIssue,
    };
    transition_response(&data, symbol, |s| s.halt(reason)).await
}

#[utoipa::path(
    post,
    path = "/v1/session/{symbol}/resume",
    params(("symbol" = String, Path, description = "Ticker symbol")),
    responses(
        (status = 200, description = "Resulting state (Open if it was Halted, otherwise unchanged)", body = SessionStateResponse),
        (status = 404, description = "Symbol not found", body = ErrorResponse),
    ),
    tag = "session"
)]
pub async fn resume_session(
    path: web::Path<String>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let symbol = path.into_inner();
    transition_response(&data, symbol, |s| s.resume()).await
}
