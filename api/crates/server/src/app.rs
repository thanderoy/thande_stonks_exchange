//! Route tables for the two listeners that share one `AppState`.
//!
//! - **Public** (`API_BIND_ADDR`): what the internet reaches through the reverse proxy.
//!   Guests join, trade as themselves and read market data. Per-IP rate limited. No
//!   session control, no Swagger.
//! - **Admin** (`ADMIN_BIND_ADDR`): the full API, including session control and Swagger,
//!   with orders naming their `client_id` in the body. Bind it to localhost or a private
//!   network only — it has no authentication.

use crate::client_ip::ClientIpExtractor;
use crate::handlers::orders::ErrorResponse;
use crate::handlers::{admin, book, guest, health, market, orders, session, ws};
use crate::openapi::ApiDoc;
use actix_governor::governor::middleware::NoOpMiddleware;
use actix_governor::{Governor, GovernorConfig, GovernorConfigBuilder};
use actix_session::SessionMiddleware;
use actix_session::config::{CookieContentSecurity, PersistentSession};
use actix_session::storage::CookieSessionStore;
use actix_web::cookie::{Key, SameSite, time};
use actix_web::{HttpResponse, error, web};
use base64::Engine;
use ipnet::IpNet;
use std::time::Duration;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub const SESSION_COOKIE: &str = "tse_session";
pub const SESSION_TTL_HOURS: i64 = 24;

/// Per-IP request quotas on the public listener: one request replenishes every
/// `period`, up to `burst` in a row.
#[derive(Debug, Clone, Copy)]
pub struct IpQuota {
    pub period: Duration,
    pub burst: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct PublicLimits {
    /// Applies to every public route.
    pub general: IpQuota,
    /// Applies to `POST /v1/join` on top of `general`.
    pub join: IpQuota,
}

impl Default for PublicLimits {
    fn default() -> Self {
        Self {
            // 20 requests/s sustained, bursts of 60.
            general: IpQuota {
                period: Duration::from_millis(50),
                burst: 60,
            },
            // 5 joins per minute.
            join: IpQuota {
                period: Duration::from_secs(12),
                burst: 5,
            },
        }
    }
}

type IpGovernorConfig = GovernorConfig<ClientIpExtractor, NoOpMiddleware>;

/// Everything the public listener needs. Build it once and clone it into each worker so
/// all workers share the same rate-limit state.
#[derive(Clone)]
pub struct PublicConfig {
    session_key: Key,
    cookie_secure: bool,
    general: IpGovernorConfig,
    join: IpGovernorConfig,
}

impl PublicConfig {
    pub fn new(
        session_key: Key,
        cookie_secure: bool,
        trusted_proxies: Vec<IpNet>,
        limits: PublicLimits,
    ) -> Self {
        let extractor = ClientIpExtractor::new(trusted_proxies);
        let governor = |quota: IpQuota| {
            GovernorConfigBuilder::default()
                .period(quota.period)
                .burst_size(quota.burst)
                .key_extractor(extractor.clone())
                .finish()
                .expect("rate-limit period and burst must be non-zero")
        };
        Self {
            session_key,
            cookie_secure,
            general: governor(limits.general),
            join: governor(limits.join),
        }
    }

    fn session_middleware(&self) -> SessionMiddleware<CookieSessionStore> {
        SessionMiddleware::builder(CookieSessionStore::default(), self.session_key.clone())
            .cookie_name(SESSION_COOKIE.to_string())
            .cookie_http_only(true)
            .cookie_same_site(SameSite::Lax)
            .cookie_path("/".to_string())
            .cookie_secure(self.cookie_secure)
            .cookie_content_security(CookieContentSecurity::Private)
            .session_lifecycle(
                PersistentSession::default().session_ttl(time::Duration::hours(SESSION_TTL_HOURS)),
            )
            .build()
    }
}

fn json_error_config() -> web::JsonConfig {
    web::JsonConfig::default().error_handler(|err, _req| {
        let response = HttpResponse::BadRequest().json(ErrorResponse {
            error: format!("invalid request body: {err}"),
        });
        error::InternalError::from_response(err, response).into()
    })
}

/// Public listener routes. Register on an `App` that has `web::Data<Arc<AppState>>`.
pub fn configure_public(cfg: &mut web::ServiceConfig, config: &PublicConfig) {
    cfg.service(
        web::scope("")
            .wrap(config.session_middleware())
            .wrap(Governor::new(&config.general))
            .app_data(json_error_config())
            .route("/healthz", web::get().to(health::healthz))
            .service(
                web::resource("/v1/join")
                    .wrap(Governor::new(&config.join))
                    .route(web::post().to(guest::join)),
            )
            .route("/v1/me", web::get().to(guest::me))
            .route("/v1/me/orders", web::get().to(guest::my_orders))
            .route("/v1/me/account", web::get().to(guest::my_account))
            .route("/v1/orders", web::post().to(orders::submit_order_public))
            .route(
                "/v1/orders/{id}",
                web::delete().to(orders::cancel_order_public),
            )
            .route("/v1/book/{symbol}", web::get().to(book::get_book))
            .route("/v1/session/{symbol}", web::get().to(session::get_session))
            .route("/v1/symbols", web::get().to(market::symbols))
            .route("/v1/trades/{symbol}", web::get().to(market::trades))
            .route("/v1/leaderboard", web::get().to(market::leaderboard_public))
            .route("/v1/stream", web::get().to(ws::stream_public)),
    );
}

/// Admin listener routes: the full API plus Swagger UI. Register on an `App` that has
/// `web::Data<Arc<AppState>>`.
pub fn configure_admin(cfg: &mut web::ServiceConfig) {
    cfg.service(
        SwaggerUi::new("/swagger-ui/{_:.*}").url("/api-docs/openapi.json", ApiDoc::openapi()),
    )
    .app_data(json_error_config())
    .route("/healthz", web::get().to(health::healthz))
    .route("/v1/orders", web::post().to(orders::submit_order))
    .route("/v1/orders/{id}", web::delete().to(orders::cancel_order))
    .route("/v1/session/{symbol}", web::get().to(session::get_session))
    .route(
        "/v1/session/{symbol}/open",
        web::post().to(session::open_session),
    )
    .route(
        "/v1/session/{symbol}/halt",
        web::post().to(session::halt_session),
    )
    .route(
        "/v1/session/{symbol}/resume",
        web::post().to(session::resume_session),
    )
    .route("/v1/book/{symbol}", web::get().to(book::get_book))
    .route("/v1/symbols", web::get().to(market::symbols))
    .route("/v1/trades/{symbol}", web::get().to(market::trades))
    .route("/v1/leaderboard", web::get().to(market::leaderboard))
    .route("/v1/admin/reset", web::post().to(admin::reset))
    .route("/v1/stream", web::get().to(ws::stream));
}

/// Parses `SESSION_KEY`: hex or base64 (standard or URL-safe, padded or not), at least
/// 64 bytes.
pub fn parse_session_key(raw: &str) -> Result<Key, String> {
    let raw = raw.trim();
    let bytes = if raw.len().is_multiple_of(2) && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        hex::decode(raw).map_err(|e| format!("SESSION_KEY is not valid hex: {e}"))?
    } else {
        use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
        STANDARD
            .decode(raw)
            .or_else(|_| URL_SAFE_NO_PAD.decode(raw.trim_end_matches('=')))
            .map_err(|_| "SESSION_KEY must be hex or base64".to_string())?
    };
    Key::try_from(bytes.as_slice()).map_err(|_| {
        format!(
            "SESSION_KEY must decode to at least 64 bytes (got {})",
            bytes.len()
        )
    })
}

/// `COOKIE_SECURE`: cookies are `Secure` unless this is `false` or `0`.
pub fn parse_cookie_secure(raw: Option<&str>) -> bool {
    !matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("false" | "0")
    )
}
