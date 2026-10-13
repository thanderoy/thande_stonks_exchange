use crate::handlers::guest;
use crate::state::AppState;
use actix_session::Session;
use actix_web::http::StatusCode;
use actix_web::{HttpResponse, web};
use chrono::Utc;
use common::{OrderId, Symbol};
use engine::matcher::{Trade, fill_outcome};
use engine::order::{Order, OrderStatus, OrderType, Side, TimeInForce};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

#[derive(Deserialize, ToSchema)]
pub struct SubmitOrderRequest {
    /// Ticker symbol, e.g. "AAPL"
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    /// Order quantity (decimal string)
    pub quantity: Decimal,
    pub time_in_force: TimeInForce,
    /// Client to trade as. Required on the admin listener; ignored on the public listener,
    /// where the identity comes from the `tse_session` cookie set by `POST /v1/join`.
    #[serde(default)]
    pub client_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SubmitOrderResponse {
    /// Server-assigned order UUID
    pub order_id: OrderId,
    /// Trades generated immediately by this order (may be empty)
    pub trades: Vec<Trade>,
    /// Quantity not filled by those trades
    pub remaining: Decimal,
    /// Whether the remainder is resting on the book, or was dropped (market, IOC, FOK)
    pub status: OrderStatus,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize, ToSchema)]
pub struct CancelOrderResponse {
    pub cancelled: OrderId,
}

#[derive(Deserialize, IntoParams)]
pub struct CancelQuery {
    /// Ticker symbol of the order to cancel
    pub symbol: String,
    pub side: Side,
}

#[utoipa::path(
    post,
    path = "/v1/orders",
    request_body = SubmitOrderRequest,
    responses(
        (status = 200, description = "Order accepted; any immediate fills are in `trades` (always empty before the open, when orders only rest)", body = SubmitOrderResponse),
        (status = 400, description = "Risk check failed, malformed request, missing `client_id` (admin), or a non-resting order before the open", body = ErrorResponse),
        (status = 401, description = "Public listener: no guest session", body = ErrorResponse),
        (status = 404, description = "Symbol not registered", body = ErrorResponse),
        (status = 409, description = "Session is Halted, PostMarket or Closed", body = ErrorResponse),
        (status = 429, description = "Participant's orders-per-second limit exceeded (or, on the public listener, the per-IP request limit)", body = ErrorResponse),
        (status = 500, description = "Internal engine error", body = ErrorResponse),
    ),
    tag = "orders"
)]
pub async fn submit_order(
    body: web::Json<SubmitOrderRequest>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let Some(client_id) = body.client_id.clone() else {
        return error(StatusCode::BAD_REQUEST, "client_id is required");
    };
    submit_as(&body, client_id, &data).await
}

/// Public listener: trades as the session's guest; any `client_id` in the body is ignored.
pub async fn submit_order_public(
    body: web::Json<SubmitOrderRequest>,
    session: Session,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let Some(me) = guest::current_participant(&session, &data) else {
        return guest::unauthorized();
    };
    submit_as(&body, me.client_id, &data).await
}

fn build_order(body: &SubmitOrderRequest, client_id: String) -> Order {
    let symbol = Symbol::new(&body.symbol);
    match &body.order_type {
        OrderType::Market => Order::new_market(symbol, body.side.clone(), body.quantity, client_id),
        OrderType::Limit { price } => Order::new_limit(
            symbol,
            body.side.clone(),
            *price,
            body.quantity,
            body.time_in_force.clone(),
            client_id,
        ),
        OrderType::StopLimit {
            stop_price,
            limit_price,
        } => {
            let now = Utc::now();
            Order {
                id: OrderId::new(),
                symbol,
                side: body.side.clone(),
                order_type: OrderType::StopLimit {
                    stop_price: *stop_price,
                    limit_price: *limit_price,
                },
                quantity: body.quantity,
                filled_quantity: Decimal::ZERO,
                time_in_force: body.time_in_force.clone(),
                created_at: now,
                updated_at: now,
                submitted_at: now,
                cancelled_at: None,
                client_id,
            }
        }
    }
}

async fn submit_as(body: &SubmitOrderRequest, client_id: String, data: &AppState) -> HttpResponse {
    let order = build_order(body, client_id);
    let trades = match data.submit(order.clone()).await {
        Ok(trades) => trades,
        Err(e) => return error(e.status(), e.to_string()),
    };
    data.persist_trades(&trades).await;
    let (remaining, status) = fill_outcome(&order, &trades);
    HttpResponse::Ok().json(SubmitOrderResponse {
        order_id: order.id,
        trades,
        remaining,
        status,
    })
}

pub(crate) fn error(status: StatusCode, message: impl Into<String>) -> HttpResponse {
    HttpResponse::build(status).json(ErrorResponse {
        error: message.into(),
    })
}

#[utoipa::path(
    delete,
    path = "/v1/orders/{id}",
    params(
        ("id" = String, Path, description = "Order UUID to cancel"),
        CancelQuery,
    ),
    responses(
        (status = 200, description = "Order cancelled", body = CancelOrderResponse),
        (status = 400, description = "Invalid order UUID", body = ErrorResponse),
        (status = 401, description = "Public listener: no guest session", body = ErrorResponse),
        (status = 404, description = "Order or symbol not found (on the public listener, also another participant's order)", body = ErrorResponse),
        (status = 500, description = "Internal engine error", body = ErrorResponse),
    ),
    tag = "orders"
)]
pub async fn cancel_order(
    path: web::Path<String>,
    query: web::Query<CancelQuery>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    cancel(&path, &query, None, &data).await
}

/// Public listener: cancels only the session guest's own orders. Anyone else's order
/// gets the same 404 as an order that doesn't exist.
pub async fn cancel_order_public(
    path: web::Path<String>,
    query: web::Query<CancelQuery>,
    session: Session,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let Some(me) = guest::current_participant(&session, &data) else {
        return guest::unauthorized();
    };
    cancel(&path, &query, Some(&me.client_id), &data).await
}

async fn cancel(
    order_id: &str,
    query: &CancelQuery,
    owner: Option<&str>,
    data: &AppState,
) -> HttpResponse {
    let Ok(uuid) = order_id.parse::<uuid::Uuid>() else {
        return error(StatusCode::BAD_REQUEST, "invalid order id");
    };
    let order_id = OrderId(uuid);
    let symbol = query.symbol.as_str();
    let side = query.side.clone();

    let result = match owner {
        Some(owner) => data.cancel_owned(order_id, symbol, side, owner).await,
        None => data.cancel(order_id, symbol, side).await,
    };
    match result {
        Ok(Some(_)) => HttpResponse::Ok().json(CancelOrderResponse {
            cancelled: order_id,
        }),
        Ok(None) => error(StatusCode::NOT_FOUND, "order not found"),
        Err(e) => error(e.status(), e.to_string()),
    }
}
