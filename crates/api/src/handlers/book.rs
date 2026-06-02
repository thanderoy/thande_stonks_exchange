use crate::state::AppState;
use actix_web::{HttpResponse, web};
use engine::shard::{BookSnapshot, EngineCommand};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::mpsc;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct BookResponse {
    pub symbol: String,
    /// [price, total_qty] pairs, best ask first (ascending)
    pub asks: Vec<[String; 2]>,
    /// [price, total_qty] pairs, best bid first (descending)
    pub bids: Vec<[String; 2]>,
}

#[utoipa::path(
    get,
    path = "/v1/book/{symbol}",
    params(("symbol" = String, Path, description = "Ticker symbol")),
    responses(
        (status = 200, description = "Top 10 price levels each side", body = BookResponse),
        (status = 404, description = "Symbol not registered"),
        (status = 500, description = "Engine error"),
    ),
    tag = "book"
)]
pub async fn get_book(
    path: web::Path<String>,
    data: web::Data<Arc<AppState>>,
) -> HttpResponse {
    let symbol_str = path.into_inner();
    let shard = match data.shards.get(&symbol_str) {
        Some(s) => s.clone(),
        None => return HttpResponse::NotFound().finish(),
    };

    let (reply_tx, mut reply_rx) = mpsc::channel::<BookSnapshot>(1);
    if shard.send(EngineCommand::GetBook(reply_tx)).await.is_err() {
        return HttpResponse::InternalServerError().finish();
    }

    let snapshot = reply_rx.recv().await.unwrap_or(BookSnapshot {
        bids: vec![],
        asks: vec![],
    });

    HttpResponse::Ok().json(BookResponse {
        symbol: symbol_str,
        bids: snapshot
            .bids
            .iter()
            .map(|(p, q)| [p.to_string(), q.to_string()])
            .collect(),
        asks: snapshot
            .asks
            .iter()
            .map(|(p, q)| [p.to_string(), q.to_string()])
            .collect(),
    })
}
