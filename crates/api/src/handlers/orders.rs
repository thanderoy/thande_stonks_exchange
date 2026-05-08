use std::sync::Arc;
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use common::{OrderId, Symbol};
use engine::order::{Order, OrderType, Side, TimeInForce};
use engine::shard::EngineCommand;
use engine::matcher::Trade;
use market_data::MarketEvent;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SubmitOrderRequest {
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub time_in_force: TimeInForce,
    pub client_id: String,
}

#[derive(Serialize)]
pub struct SubmitOrderResponse {
    pub order_id: OrderId,
    pub trades: Vec<Trade>,
}

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
        OrderType::StopLimit { stop_price, limit_price } => {
            use chrono::Utc;
            let now = Utc::now();
            Order {
                id: OrderId::new(),
                symbol: symbol.clone(),
                side: body.side.clone(),
                order_type: OrderType::StopLimit { stop_price: *stop_price, limit_price: *limit_price },
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

    if let Err(e) = data.risk.check(&order) {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() }));
    }

    let shard = match data.shards.get(&body.symbol) {
        Some(s) => s.clone(),
        None => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({ "error": format!("symbol {} not found", body.symbol) }))
        }
    };

    let (reply_tx, mut reply_rx) = mpsc::channel::<Vec<Trade>>(1);
    if shard.send(EngineCommand::Submit(order, reply_tx)).await.is_err() {
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "shard unavailable" }));
    }

    let trades = reply_rx.recv().await.unwrap_or_default();

    for trade in &trades {
        data.bus.publish(MarketEvent::Trade(trade.clone()));

        if let Some(pool) = &data.pool {
            let _ = persistence::ledger::persist_trade(pool, trade).await;
        }
    }

    data.bus.publish(MarketEvent::OrderAccepted {
        order_id,
        symbol,
    });

    HttpResponse::Ok().json(SubmitOrderResponse { order_id, trades })
}

#[derive(Deserialize)]
pub struct CancelQuery {
    pub symbol: String,
    pub side: Side,
}

pub async fn cancel_order(
    path: web::Path<String>,
    query: web::Query<CancelQuery>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let order_id_str = path.into_inner();
    let uuid = match order_id_str.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": "invalid order id" }))
        }
    };
    let order_id = OrderId(uuid);
    let symbol_str = query.symbol.clone();

    let shard = match data.shards.get(&symbol_str) {
        Some(s) => s.clone(),
        None => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({ "error": format!("symbol {} not found", symbol_str) }))
        }
    };

    let (reply_tx, mut reply_rx) = mpsc::channel::<Option<Order>>(1);
    if shard
        .send(EngineCommand::Cancel(order_id, query.side.clone(), reply_tx))
        .await
        .is_err()
    {
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "shard unavailable" }));
    }

    match reply_rx.recv().await.flatten() {
        Some(_) => {
            data.bus.publish(MarketEvent::OrderCancelled {
                order_id,
                symbol: Symbol::new(&symbol_str),
            });
            HttpResponse::Ok().json(serde_json::json!({ "cancelled": order_id }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({ "error": "order not found" })),
    }
}
