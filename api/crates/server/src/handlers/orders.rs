use crate::state::AppState;
use actix_web::{HttpResponse, web};
use common::{OrderId, Symbol};
use engine::matcher::Trade;
use engine::order::{Order, OrderType, Side, TimeInForce};
use market_data::MarketEvent;
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
    /// Pre-registered client identifier
    pub client_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct SubmitOrderResponse {
    /// Server-assigned order UUID
    pub order_id: OrderId,
    /// Trades generated immediately by this order (may be empty)
    pub trades: Vec<Trade>,
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
        (status = 400, description = "Risk check failed, malformed request, or a non-resting order before the open", body = ErrorResponse),
        (status = 404, description = "Symbol not registered", body = ErrorResponse),
        (status = 409, description = "Session is Halted, PostMarket or Closed", body = ErrorResponse),
        (status = 500, description = "Internal engine error", body = ErrorResponse),
    ),
    tag = "orders"
)]
pub async fn submit_order(
    body: web::Json<SubmitOrderRequest>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let symbol = Symbol::new(&body.symbol);

    let order = match &body.order_type {
        OrderType::Market => Order::new_market(
            symbol.clone(),
            body.side.clone(),
            body.quantity,
            &body.client_id,
        ),
        OrderType::Limit { price } => Order::new_limit(
            symbol.clone(),
            body.side.clone(),
            *price,
            body.quantity,
            body.time_in_force.clone(),
            &body.client_id,
        ),
        OrderType::StopLimit {
            stop_price,
            limit_price,
        } => {
            use chrono::Utc;
            let now = Utc::now();
            Order {
                id: OrderId::new(),
                symbol: symbol.clone(),
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
                client_id: body.client_id.clone(),
            }
        }
    };

    let order_id = order.id;

    let trades = match data.submit(order).await {
        Ok(trades) => trades,
        Err(e) => {
            return HttpResponse::build(e.status()).json(ErrorResponse {
                error: e.to_string(),
            });
        }
    };

    data.emit_trades(&trades).await;
    data.bus
        .publish(MarketEvent::OrderAccepted { order_id, symbol });

    HttpResponse::Ok().json(SubmitOrderResponse { order_id, trades })
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
        (status = 404, description = "Order or symbol not found", body = ErrorResponse),
        (status = 500, description = "Internal engine error", body = ErrorResponse),
    ),
    tag = "orders"
)]
pub async fn cancel_order(
    path: web::Path<String>,
    query: web::Query<CancelQuery>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let order_id_str = path.into_inner();
    let uuid = match order_id_str.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => {
            return HttpResponse::BadRequest().json(ErrorResponse {
                error: "invalid order id".into(),
            });
        }
    };
    let order_id = OrderId(uuid);
    let symbol_str = query.symbol.clone();

    let removed = match data.cancel(order_id, &symbol_str, query.side.clone()).await {
        Ok(removed) => removed,
        Err(e) => {
            return HttpResponse::build(e.status()).json(ErrorResponse {
                error: e.to_string(),
            });
        }
    };

    match removed {
        Some(_) => {
            data.bus.publish(MarketEvent::OrderCancelled {
                order_id,
                symbol: Symbol::new(&symbol_str),
            });
            HttpResponse::Ok().json(CancelOrderResponse {
                cancelled: order_id,
            })
        }
        None => HttpResponse::NotFound().json(ErrorResponse {
            error: "order not found".into(),
        }),
    }
}
