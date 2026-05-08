use std::sync::Arc;
use actix_web::{web, HttpResponse};
use engine::session::HaltReason;
use rust_decimal::Decimal;
use serde::Deserialize;
use crate::state::AppState;

pub async fn get_session(
    path: web::Path<String>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let symbol = path.into_inner();
    match data.sessions.get(&symbol) {
        Some(session) => {
            let state = session.lock().unwrap().state.clone();
            HttpResponse::Ok().json(serde_json::json!({ "symbol": symbol, "state": state }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({ "error": "symbol not found" })),
    }
}

pub async fn open_session(
    path: web::Path<String>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let symbol = path.into_inner();
    match data.sessions.get(&symbol) {
        Some(session) => {
            session.lock().unwrap().open();
            HttpResponse::Ok().json(serde_json::json!({ "symbol": symbol, "state": "Open" }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({ "error": "symbol not found" })),
    }
}

#[derive(Deserialize)]
pub struct HaltBody {
    #[serde(default)]
    pub reason: Option<String>,
    pub threshold_pct: Option<Decimal>,
}

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
    match data.sessions.get(&symbol) {
        Some(session) => {
            session.lock().unwrap().halt(reason);
            HttpResponse::Ok().json(serde_json::json!({ "symbol": symbol, "state": "Halted" }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({ "error": "symbol not found" })),
    }
}

pub async fn resume_session(
    path: web::Path<String>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let symbol = path.into_inner();
    match data.sessions.get(&symbol) {
        Some(session) => {
            session.lock().unwrap().resume();
            HttpResponse::Ok().json(serde_json::json!({ "symbol": symbol, "state": "Open" }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({ "error": "symbol not found" })),
    }
}
