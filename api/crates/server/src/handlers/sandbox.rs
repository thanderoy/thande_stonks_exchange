//! The caller's sandbox, under `/v1/me/sandbox` on the public listener.

use crate::handlers::guest;
use crate::handlers::orders::ErrorResponse;
use crate::state::AppState;
use actix_session::Session;
use actix_web::{HttpResponse, web};
use std::sync::Arc;

fn no_sandbox() -> HttpResponse {
    HttpResponse::NotFound().json(ErrorResponse {
        error: "you have no sandbox; POST /v1/me/sandbox to create one".into(),
    })
}

fn failed(e: crate::exchange::ExchangeError) -> HttpResponse {
    HttpResponse::build(e.status()).json(ErrorResponse {
        error: e.to_string(),
    })
}

#[utoipa::path(
    get,
    path = "/v1/me/sandbox",
    responses(
        (status = 200, description = "The caller's sandbox. Public listener only.", body = crate::sandbox::SandboxResponse),
        (status = 401, description = "No valid guest session", body = ErrorResponse),
        (status = 404, description = "The caller has no sandbox", body = ErrorResponse),
    ),
    tag = "sandbox"
)]
pub async fn get(session: Session, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let Some(me) = guest::current_participant(&session, &data) else {
        return guest::unauthorized();
    };
    let view = match data.sandbox_of(&me.client_id) {
        Some(symbol) => data.sandbox_view(&symbol).await,
        None => None,
    };
    match view {
        Some(view) => HttpResponse::Ok().json(view),
        None => no_sandbox(),
    }
}

#[utoipa::path(
    post,
    path = "/v1/me/sandbox",
    responses(
        (status = 200, description = "The caller's sandbox, created in pre-market if they had none. Only they can see it, trade it, and open, halt or resume it (`POST /v1/session/{symbol}/open` etc.). It is removed after 30 idle minutes and at every market reset. Public listener only.", body = crate::sandbox::SandboxResponse),
        (status = 401, description = "No valid guest session", body = ErrorResponse),
        (status = 503, description = "Every sandbox slot is in use", body = ErrorResponse),
    ),
    tag = "sandbox"
)]
pub async fn create(session: Session, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let Some(me) = guest::current_participant(&session, &data) else {
        return guest::unauthorized();
    };
    let symbol = match data.create_sandbox(&me.client_id) {
        Ok(symbol) => symbol,
        Err(e) => return failed(e),
    };
    match data.sandbox_view(&symbol).await {
        Some(view) => HttpResponse::Ok().json(view),
        None => no_sandbox(),
    }
}

#[utoipa::path(
    delete,
    path = "/v1/me/sandbox",
    responses(
        (status = 204, description = "Sandbox removed with its book. Public listener only."),
        (status = 401, description = "No valid guest session", body = ErrorResponse),
        (status = 404, description = "The caller has no sandbox", body = ErrorResponse),
    ),
    tag = "sandbox"
)]
pub async fn delete(session: Session, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let Some(me) = guest::current_participant(&session, &data) else {
        return guest::unauthorized();
    };
    let Some(symbol) = data.sandbox_of(&me.client_id) else {
        return no_sandbox();
    };
    match data.remove_sandbox(&symbol, "deleted").await {
        Ok(true) => HttpResponse::NoContent().finish(),
        Ok(false) => no_sandbox(),
        Err(e) => failed(e),
    }
}

#[utoipa::path(
    post,
    path = "/v1/me/sandbox/seed",
    responses(
        (status = 200, description = "The maker bot placed six sample orders in the caller's sandbox: crossed before the open (so opening runs an auction), a two-sided book once open. Public listener only.", body = crate::sandbox::SeedResponse),
        (status = 401, description = "No valid guest session", body = ErrorResponse),
        (status = 404, description = "The caller has no sandbox", body = ErrorResponse),
        (status = 409, description = "The sandbox is halted, or already holds plenty of sample orders", body = ErrorResponse),
    ),
    tag = "sandbox"
)]
pub async fn seed(session: Session, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let Some(me) = guest::current_participant(&session, &data) else {
        return guest::unauthorized();
    };
    let Some(symbol) = data.sandbox_of(&me.client_id) else {
        return no_sandbox();
    };
    match data.seed_sandbox(&symbol).await {
        Ok(seeded) => HttpResponse::Ok().json(seeded),
        Err(e) => failed(e),
    }
}
