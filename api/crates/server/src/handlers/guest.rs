//! `POST /v1/join` and `GET /v1/me` on the public listener.

use crate::exchange::Participant;
use crate::guest;
use crate::handlers::orders::ErrorResponse;
use crate::state::AppState;
use actix_session::Session;
use actix_web::{HttpResponse, web};
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
