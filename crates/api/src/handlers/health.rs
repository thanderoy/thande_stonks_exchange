use actix_web::HttpResponse;

pub async fn healthz() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}
