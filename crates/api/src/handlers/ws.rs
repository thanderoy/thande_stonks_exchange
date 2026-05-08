use std::sync::Arc;
use actix_web::{web, HttpRequest, HttpResponse};
use crate::state::AppState;

pub async fn stream(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<Arc<AppState>>,
) -> Result<HttpResponse, actix_web::Error> {
    let (res, mut session, _msg_stream) = actix_ws::handle(&req, stream)?;
    let mut rx = data.bus.subscribe();

    actix_web::rt::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    if session.text(json).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    Ok(res)
}
