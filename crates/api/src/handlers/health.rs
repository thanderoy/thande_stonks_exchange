use actix_web::HttpResponse;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

#[utoipa::path(
    get,
    path = "/healthz",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
    ),
    tag = "health"
)]
pub async fn healthz() -> HttpResponse {
    HttpResponse::Ok().json(HealthResponse {
        status: "ok".into(),
    })
}
