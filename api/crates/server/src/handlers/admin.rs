//! Admin-only market operations.

use crate::app::SESSION_TTL_HOURS;
use crate::handlers::orders::ErrorResponse;
use crate::state::AppState;
use actix_web::{HttpResponse, web};
use chrono::{Duration, Utc};
use std::sync::Arc;

#[utoipa::path(
    post,
    path = "/v1/admin/reset",
    responses(
        (status = 200, description = "Market reset: books, positions, cash and trade history cleared, every symbol back in pre-market, a fresh WAL started (the old one is kept as `<WAL_PATH>.prev`). Guests who joined in the last 24 hours keep their accounts.", body = ResetSummary),
        (status = 500, description = "Reset failed (WAL rotation or engine error)", body = ErrorResponse),
    ),
    tag = "admin"
)]
pub async fn reset(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cutoff = Utc::now() - Duration::hours(SESSION_TTL_HOURS);
    match data.reset_market(cutoff).await {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => HttpResponse::build(e.status()).json(ErrorResponse {
            error: e.to_string(),
        }),
    }
}
