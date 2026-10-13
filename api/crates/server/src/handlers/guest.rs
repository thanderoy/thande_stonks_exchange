//! `POST /v1/join` and the caller's own data under `/v1/me` on the public listener.

use crate::exchange::Participant;
use crate::guest;
use crate::handlers::orders::ErrorResponse;
use crate::sandbox::Viewer;
use crate::state::AppState;
use actix_session::Session;
use actix_web::{HttpResponse, web};
use chrono::{DateTime, Utc};
use common::OrderId;
use engine::order::{OrderType, Side, TimeInForce};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

/// Session key holding the guest's client id.
const SESSION_CLIENT_ID: &str = "client_id";

#[derive(Deserialize, ToSchema)]
pub struct JoinRequest {
    /// 2-24 characters after trimming: letters, digits, spaces, `_` and `-`
    pub display_name: String,
}

#[derive(Serialize, ToSchema)]
pub struct IdentityResponse {
    /// Server-assigned guest id, `g-` followed by ten lowercase alphanumerics
    pub client_id: String,
    pub display_name: String,
}

impl From<Participant> for IdentityResponse {
    fn from(p: Participant) -> Self {
        Self {
            client_id: p.client_id,
            display_name: p.display_name,
        }
    }
}

/// The participant this request's session belongs to, if the session is valid and the
/// participant is still registered.
pub fn current_participant(session: &Session, state: &AppState) -> Option<Participant> {
    let client_id = session.get::<String>(SESSION_CLIENT_ID).ok().flatten()?;
    state.participant(&client_id)
}

/// Whether the public caller may see `symbol`: shared symbols always, a sandbox only
/// when it is theirs.
pub fn can_see(session: &Session, state: &AppState, symbol: &str) -> bool {
    let me = current_participant(session, state);
    state.visible_to(
        symbol,
        Viewer::Public(me.as_ref().map(|p| p.client_id.as_str())),
    )
}

/// The 404 a hidden sandbox gets, the same as for a symbol that doesn't exist.
pub fn symbol_not_found(symbol: &str) -> HttpResponse {
    HttpResponse::NotFound().json(ErrorResponse {
        error: format!("symbol {symbol} not found"),
    })
}

pub fn unauthorized() -> HttpResponse {
    HttpResponse::Unauthorized().json(ErrorResponse {
        error: "no guest session; POST /v1/join first".into(),
    })
}

#[utoipa::path(
    post,
    path = "/v1/join",
    request_body = JoinRequest,
    responses(
        (status = 200, description = "Guest created (or the caller's existing guest returned); sets the `tse_session` cookie. Public listener only.", body = IdentityResponse),
        (status = 400, description = "Invalid display name", body = ErrorResponse),
        (status = 429, description = "Too many joins from this address", body = ErrorResponse),
    ),
    tag = "guest"
)]
pub async fn join(
    body: web::Json<JoinRequest>,
    session: Session,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    if let Some(me) = current_participant(&session, &data) {
        return HttpResponse::Ok().json(IdentityResponse::from(me));
    }
    let display_name = match guest::validate_display_name(&body.display_name) {
        Ok(name) => name,
        Err(message) => return HttpResponse::BadRequest().json(ErrorResponse { error: message }),
    };
    let participant = match data.register_participant(
        display_name,
        guest::max_notional_per_order(),
        guest::max_position(),
        guest::MAX_ORDERS_PER_SECOND,
    ) {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::build(e.status()).json(ErrorResponse {
                error: e.to_string(),
            });
        }
    };
    if let Err(e) = session.insert(SESSION_CLIENT_ID, &participant.client_id) {
        tracing::error!("could not store guest session: {e}");
        return HttpResponse::InternalServerError().json(ErrorResponse {
            error: "could not create session".into(),
        });
    }
    HttpResponse::Ok().json(IdentityResponse::from(participant))
}

#[utoipa::path(
    get,
    path = "/v1/me",
    responses(
        (status = 200, description = "The caller's guest identity. Public listener only.", body = IdentityResponse),
        (status = 401, description = "No valid guest session", body = ErrorResponse),
    ),
    tag = "guest"
)]
pub async fn me(session: Session, data: web::Data<Arc<AppState>>) -> HttpResponse {
    match current_participant(&session, &data) {
        Some(me) => HttpResponse::Ok().json(IdentityResponse::from(me)),
        None => unauthorized(),
    }
}

#[derive(Serialize, ToSchema)]
pub struct OpenOrder {
    pub order_id: OrderId,
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    /// Quantity still resting
    pub remaining: Decimal,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct OpenOrdersResponse {
    /// Bids then asks per symbol, best price and earliest first
    pub orders: Vec<OpenOrder>,
}

#[utoipa::path(
    get,
    path = "/v1/me/orders",
    responses(
        (status = 200, description = "The caller's resting orders. Public listener only.", body = OpenOrdersResponse),
        (status = 401, description = "No valid guest session", body = ErrorResponse),
    ),
    tag = "guest"
)]
pub async fn my_orders(session: Session, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let Some(me) = current_participant(&session, &data) else {
        return unauthorized();
    };
    let orders = data
        .open_orders(Some(&me.client_id))
        .await
        .into_iter()
        .map(|o| OpenOrder {
            order_id: o.id,
            symbol: o.symbol.0,
            side: o.side,
            order_type: o.order_type,
            time_in_force: o.time_in_force,
            remaining: o.quantity - o.filled_quantity,
            submitted_at: o.submitted_at,
        })
        .collect();
    HttpResponse::Ok().json(OpenOrdersResponse { orders })
}

#[utoipa::path(
    get,
    path = "/v1/me/account",
    responses(
        (status = 200, description = "The caller's cash, positions and P&L since the last reset. Public listener only.", body = Account),
        (status = 401, description = "No valid guest session", body = ErrorResponse),
    ),
    tag = "guest"
)]
pub async fn my_account(session: Session, data: web::Data<Arc<AppState>>) -> HttpResponse {
    let account = current_participant(&session, &data).and_then(|me| data.account(&me.client_id));
    match account {
        Some(account) => HttpResponse::Ok().json(account),
        None => unauthorized(),
    }
}
