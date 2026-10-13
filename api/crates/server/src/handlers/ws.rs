use crate::handlers::guest;
use crate::state::AppState;
use actix_session::Session;
use actix_web::{HttpRequest, HttpResponse, web};
use actix_ws::Message;
use market_data::MarketEvent;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Admin listener: every event, including every participant's order events.
pub async fn stream(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<Arc<AppState>>,
) -> Result<HttpResponse, actix_web::Error> {
    serve(req, stream, &data, None, true)
}

/// Public listener: market events for everyone, plus `order_accepted` and
/// `order_cancelled` for the session's own guest only. Identity is fixed when the socket
/// opens, so a visitor who joins afterwards reconnects to get their order events.
pub async fn stream_public(
    req: HttpRequest,
    stream: web::Payload,
    session: Session,
    data: web::Data<Arc<AppState>>,
) -> Result<HttpResponse, actix_web::Error> {
    let me = guest::current_participant(&session, &data).map(|p| p.client_id);
    serve(req, stream, &data, me, false)
}

/// Whether a stream connection for `viewer` gets `event`: public events go to everyone,
/// private ones only to their owner (or to every connection when `see_all`).
pub fn forwarded(event: &MarketEvent, viewer: Option<&str>, see_all: bool) -> bool {
    match event.owner() {
        None => true,
        Some(owner) => see_all || viewer == Some(owner),
    }
}

fn serve(
    req: HttpRequest,
    stream: web::Payload,
    data: &AppState,
    viewer: Option<String>,
    see_all: bool,
) -> Result<HttpResponse, actix_web::Error> {
    let (res, mut session, mut incoming) = actix_ws::handle(&req, stream)?;
    let mut rx = data.bus.subscribe();

    actix_web::rt::spawn(async move {
        let reason = loop {
            tokio::select! {
                event = rx.recv() => match event {
                    Ok(event) => {
                        if !forwarded(&event, viewer.as_deref(), see_all) {
                            continue;
                        }
                        let json = serde_json::to_string(&event).unwrap_or_default();
                        if session.text(json).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break None,
                },
                // Clients send nothing but control frames: answer pings, honour closes.
                message = incoming.recv() => match message {
                    Some(Ok(Message::Ping(bytes))) => {
                        if session.pong(&bytes).await.is_err() {
                            return;
                        }
                    }
                    Some(Ok(Message::Close(reason))) => break reason,
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break None,
                },
            }
        };
        let _ = session.close(reason).await;
    });

    Ok(res)
}
