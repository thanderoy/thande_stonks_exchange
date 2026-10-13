//! HTTP-level tests of the public and admin listener configurations.

use actix_web::body::MessageBody;
use actix_web::cookie::{Cookie, Key, SameSite};
use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::{Method, StatusCode};
use actix_web::{App, test, web};
use common::Symbol;
use engine::order::Side;
use engine::shard::{BookSnapshot, EngineCommand};
use market_data::EventBus;
use persistence::wal::WriteAheadLog;
use risk::{ParticipantLimits, RiskEngine, SymbolLimits};
use rust_decimal_macros::dec;
use serde_json::{Value, json};
use server::app::{PublicConfig, PublicLimits, SESSION_COOKIE, configure_admin, configure_public};
use server::state::AppState;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::mpsc;

const SYM: &str = "AAPL";

fn state() -> Arc<AppState> {
    let participant_limits = ["alice", "bob"]
        .into_iter()
        .map(|name| {
            (
                name.to_string(),
                ParticipantLimits {
                    max_notional_per_order: dec!(1_000_000),
                    max_orders_per_second: 1000,
                    current_position: HashMap::new(),
                    max_position: dec!(1000),
                },
            )
        })
        .collect();
    let symbol_limits = HashMap::from([(
        Symbol::new(SYM),
        SymbolLimits {
            active: true,
            price_collar_pct: dec!(20),
            reference_price: dec!(100),
        },
    )]);
    Arc::new(AppState::new(
        &[SYM.to_string()],
        RiskEngine::new(participant_limits, symbol_limits),
        Arc::new(EventBus::new(64)),
        None,
    ))
}

fn public_config(key: Key) -> PublicConfig {
    PublicConfig::new(key, true, vec![], PublicLimits::default())
}

async fn public_app(
    state: Arc<AppState>,
    config: PublicConfig,
) -> impl Service<
    actix_http::Request,
    Response = ServiceResponse<impl MessageBody>,
    Error = actix_web::Error,
> {
    test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(move |cfg| configure_public(cfg, &config)),
    )
    .await
}

async fn admin_app(
    state: Arc<AppState>,
) -> impl Service<
    actix_http::Request,
    Response = ServiceResponse<impl MessageBody>,
    Error = actix_web::Error,
> {
    test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_admin),
    )
    .await
}

fn peer(n: u8) -> SocketAddr {
    SocketAddr::from(([203, 0, 113, n], 40000))
}

fn req(method: Method, uri: &str) -> test::TestRequest {
    test::TestRequest::default()
        .method(method)
        .uri(uri)
        .peer_addr(peer(1))
}

fn session_cookie<B>(resp: &ServiceResponse<B>) -> Option<Cookie<'static>> {
    resp.response()
        .cookies()
        .find(|c| c.name() == SESSION_COOKIE)
        .map(|c| c.into_owned())
}

/// Joins from `peer(ip)` and returns the session cookie and the client id.
async fn join<S, B>(app: &S, name: &str, ip: u8) -> (Cookie<'static>, String)
where
    S: Service<actix_http::Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let resp = test::call_service(
        app,
        req(Method::POST, "/v1/join")
            .peer_addr(peer(ip))
            .set_json(json!({ "display_name": name }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = session_cookie(&resp).expect("join sets the session cookie");
    let body: Value = test::read_body_json(resp).await;
    (cookie, body["client_id"].as_str().unwrap().to_string())
}

fn limit_order(client_id: Option<&str>, side: &str, price: &str, qty: &str) -> Value {
    let mut body = json!({
        "symbol": SYM,
        "side": side,
        "order_type": { "type": "Limit", "price": price },
        "quantity": qty,
        "time_in_force": "GoodTillCanceled",
    });
    if let Some(id) = client_id {
        body["client_id"] = json!(id);
    }
    body
}

async fn book(state: &AppState) -> BookSnapshot {
    let shard = state.shards.get(SYM).unwrap().clone();
    let (tx, mut rx) = mpsc::channel(1);
    shard.send(EngineCommand::GetBook(tx)).await.unwrap();
    rx.recv().await.unwrap()
}

#[actix_web::test]
async fn join_sets_cookie_and_me_returns_identity() {
    let app = public_app(state(), public_config(Key::generate())).await;

    let resp = test::call_service(
        &app,
        req(Method::POST, "/v1/join")
            .set_json(json!({ "display_name": "  Ada Lovelace  " }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = session_cookie(&resp).unwrap();
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(
        cookie.max_age(),
        Some(actix_web::cookie::time::Duration::hours(24))
    );
    let joined: Value = test::read_body_json(resp).await;
    assert_eq!(joined["display_name"], "Ada Lovelace");
    let client_id = joined["client_id"].as_str().unwrap();
    assert_eq!(client_id.len(), 12);
    assert!(client_id.starts_with("g-"));
    assert!(
        client_id[2..]
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    );

    let resp = test::call_service(
        &app,
        req(Method::GET, "/v1/me")
            .cookie(cookie.clone())
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let me: Value = test::read_body_json(resp).await;
    assert_eq!(me, joined);

    // Joining again with a valid session returns the same identity.
    let resp = test::call_service(
        &app,
        req(Method::POST, "/v1/join")
            .cookie(cookie)
            .set_json(json!({ "display_name": "Someone Else" }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let again: Value = test::read_body_json(resp).await;
    assert_eq!(again, joined);
}

#[actix_web::test]
async fn me_without_session_is_401() {
    let app = public_app(state(), public_config(Key::generate())).await;
    let resp = test::call_service(&app, req(Method::GET, "/v1/me").to_request()).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: Value = test::read_body_json(resp).await;
    assert!(body["error"].is_string());
}

#[actix_web::test]
async fn join_rejects_invalid_display_names() {
    let app = public_app(state(), public_config(Key::generate())).await;
    for (i, name) in [
        "a",
        " x ",
        "this name is far too long!",
        "<script>",
        "émile",
    ]
    .into_iter()
    .enumerate()
    {
        let resp = test::call_service(
            &app,
            req(Method::POST, "/v1/join")
                .peer_addr(peer(10 + i as u8))
                .set_json(json!({ "display_name": name }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{name:?}");
        assert!(session_cookie(&resp).is_none());
        let body: Value = test::read_body_json(resp).await;
        assert!(
            body["error"].as_str().unwrap().contains("display_name"),
            "{body}"
        );
    }
}

#[actix_web::test]
async fn order_without_session_is_401() {
    let app = public_app(state(), public_config(Key::generate())).await;
    let resp = test::call_service(
        &app,
        req(Method::POST, "/v1/orders")
            .set_json(limit_order(Some("alice"), "Buy", "100", "1"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = test::call_service(
        &app,
        req(
            Method::DELETE,
            &format!("/v1/orders/{}?symbol={SYM}&side=Buy", uuid::Uuid::new_v4()),
        )
        .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn public_order_ignores_body_client_id() {
    let state = state();
    let app = public_app(state.clone(), public_config(Key::generate())).await;
    let (cookie, guest_id) = join(&app, "Guest", 1).await;

    let resp = test::call_service(
        &app,
        req(Method::POST, "/v1/orders")
            .cookie(cookie)
            .set_json(limit_order(Some("alice"), "Buy", "100", "3"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    let order_id = server_order_id(&body);

    // The order is the guest's, not alice's.
    assert!(
        state
            .cancel_owned(order_id, SYM, Side::Buy, "alice")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        state
            .cancel_owned(order_id, SYM, Side::Buy, &guest_id)
            .await
            .unwrap()
            .is_some()
    );
}

fn server_order_id(body: &Value) -> common::OrderId {
    common::OrderId(body["order_id"].as_str().unwrap().parse().unwrap())
}

#[actix_web::test]
async fn cancelling_another_guests_order_is_404() {
    let state = state();
    let app = public_app(state.clone(), public_config(Key::generate())).await;
    let (owner, _) = join(&app, "Owner", 1).await;
    let (other, _) = join(&app, "Other", 2).await;

    let resp = test::call_service(
        &app,
        req(Method::POST, "/v1/orders")
            .cookie(owner.clone())
            .set_json(limit_order(None, "Sell", "105", "4"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    let uri = format!(
        "/v1/orders/{}?symbol={SYM}&side=Sell",
        body["order_id"].as_str().unwrap()
    );

    let resp = test::call_service(&app, req(Method::DELETE, &uri).cookie(other).to_request()).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let err: Value = test::read_body_json(resp).await;
    assert_eq!(err["error"], "order not found");
    assert_eq!(book(&state).await.asks, vec![(dec!(105), dec!(4))]);

    let resp = test::call_service(&app, req(Method::DELETE, &uri).cookie(owner).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(book(&state).await.asks.is_empty());
}

#[actix_web::test]
async fn guest_order_rate_limit_returns_429() {
    let app = public_app(state(), public_config(Key::generate())).await;
    let (cookie, _) = join(&app, "Spammer", 1).await;
    let mut statuses = Vec::new();
    for _ in 0..6 {
        let resp = test::call_service(
            &app,
            req(Method::POST, "/v1/orders")
                .cookie(cookie.clone())
                .set_json(limit_order(None, "Buy", "100", "1"))
                .to_request(),
        )
        .await;
        statuses.push(resp.status());
    }
    assert_eq!(&statuses[..5], &[StatusCode::OK; 5]);
    assert_eq!(statuses[5], StatusCode::TOO_MANY_REQUESTS);
}

#[actix_web::test]
async fn admin_listener_accepts_body_client_id() {
    let state = state();
    let app = admin_app(state.clone()).await;
    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/v1/orders")
            .set_json(limit_order(Some("alice"), "Buy", "100", "2"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert!(
        state
            .cancel_owned(server_order_id(&body), SYM, Side::Buy, "alice")
            .await
            .unwrap()
            .is_some()
    );

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/v1/orders")
            .set_json(limit_order(None, "Buy", "100", "2"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[actix_web::test]
async fn session_control_and_swagger_are_admin_only() {
    let state = state();
    let public = public_app(state.clone(), public_config(Key::generate())).await;
    let admin = admin_app(state.clone()).await;

    for action in ["open", "halt", "resume"] {
        let uri = format!("/v1/session/{SYM}/{action}");
        let resp = test::call_service(
            &public,
            req(Method::POST, &uri).set_json(json!({})).to_request(),
        )
        .await;
        assert!(
            matches!(
                resp.status(),
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ),
            "{uri}: {}",
            resp.status()
        );
    }
    for uri in ["/swagger-ui/", "/api-docs/openapi.json"] {
        let resp = test::call_service(&public, req(Method::GET, uri).to_request()).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{uri}");
        let resp = test::call_service(&admin, test::TestRequest::get().uri(uri).to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK, "{uri}");
    }

    // Read-only session state is public; transitions work on admin.
    let resp = test::call_service(
        &public,
        req(Method::GET, &format!("/v1/session/{SYM}")).to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = test::call_service(
        &admin,
        test::TestRequest::post()
            .uri(&format!("/v1/session/{SYM}/open"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["state"], "Open");
}

#[actix_web::test]
async fn join_is_rate_limited_per_ip() {
    let app = public_app(state(), public_config(Key::generate())).await;
    for i in 0..5 {
        join(&app, &format!("Guest {i}"), 7).await;
    }
    let resp = test::call_service(
        &app,
        req(Method::POST, "/v1/join")
            .peer_addr(peer(7))
            .set_json(json!({ "display_name": "One Too Many" }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let body: Value = test::read_body_json(resp).await;
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("too many requests")
    );

    // Another address is unaffected.
    join(&app, "Elsewhere", 8).await;
}

#[actix_web::test]
async fn general_limit_applies_to_every_public_route() {
    let limits = PublicLimits {
        general: server::app::IpQuota {
            period: std::time::Duration::from_secs(60),
            burst: 2,
        },
        ..PublicLimits::default()
    };
    let app = public_app(
        state(),
        PublicConfig::new(Key::generate(), true, vec![], limits),
    )
    .await;
    let mut statuses = Vec::new();
    for _ in 0..3 {
        let resp = test::call_service(&app, req(Method::GET, "/healthz").to_request()).await;
        statuses.push(resp.status());
    }
    assert_eq!(
        statuses,
        [
            StatusCode::OK,
            StatusCode::OK,
            StatusCode::TOO_MANY_REQUESTS
        ]
    );
}

#[actix_web::test]
async fn guest_session_survives_restart_with_same_key_and_wal() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();
    let key = Key::generate();

    let live = state();
    *live.wal.lock().unwrap() = Some(WriteAheadLog::open(path).unwrap());
    let (cookie, guest_id) = {
        let app = public_app(live.clone(), public_config(key.clone())).await;
        let (cookie, guest_id) = join(&app, "Persistent", 1).await;
        let resp = test::call_service(
            &app,
            req(Method::POST, "/v1/orders")
                .cookie(cookie.clone())
                .set_json(limit_order(None, "Buy", "99", "6"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        (cookie, guest_id)
    };

    let recovered = state();
    let (_, entries) = WriteAheadLog::open_for_recovery(path).unwrap();
    recovered.replay(entries).await.unwrap();
    assert_eq!(book(&recovered).await.bids, vec![(dec!(99), dec!(6))]);

    let app = public_app(recovered, public_config(key)).await;
    let resp =
        test::call_service(&app, req(Method::GET, "/v1/me").cookie(cookie).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let me: Value = test::read_body_json(resp).await;
    assert_eq!(me["client_id"], guest_id.as_str());
    assert_eq!(me["display_name"], "Persistent");
}

async fn open_market(state: &Arc<AppState>) {
    state.transition(SYM, |t| t.open()).await.unwrap();
}

async fn get_json<S, B>(app: &S, uri: &str, cookie: Option<&Cookie<'static>>) -> (StatusCode, Value)
where
    S: Service<actix_http::Request, Response = ServiceResponse<B>, Error = actix_web::Error>,
    B: MessageBody,
{
    let mut request = req(Method::GET, uri);
    if let Some(cookie) = cookie {
        request = request.cookie(cookie.clone());
    }
    let resp = test::call_service(app, request.to_request()).await;
    let status = resp.status();
    let body = if status == StatusCode::OK {
        test::read_body_json(resp).await
    } else {
        Value::Null
    };
    (status, body)
}

#[actix_web::test]
async fn me_orders_and_account_follow_the_guests_own_activity() {
    let state = state();
    open_market(&state).await;
    let app = public_app(state.clone(), public_config(Key::generate())).await;
    let (ada, _) = join(&app, "Ada", 1).await;
    let (bo, _) = join(&app, "Bo", 2).await;

    for uri in ["/v1/me/orders", "/v1/me/account"] {
        assert_eq!(
            get_json(&app, uri, None).await.0,
            StatusCode::UNAUTHORIZED,
            "{uri}"
        );
    }

    let resp = test::call_service(
        &app,
        req(Method::POST, "/v1/orders")
            .cookie(bo.clone())
            .set_json(limit_order(None, "Sell", "100.50", "10"))
            .to_request(),
    )
    .await;
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(
        (body["remaining"].as_str(), body["status"].as_str()),
        (Some("10"), Some("resting"))
    );

    let resp = test::call_service(
        &app,
        req(Method::POST, "/v1/orders")
            .cookie(ada.clone())
            .set_json(limit_order(None, "Buy", "100.50", "4"))
            .to_request(),
    )
    .await;
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(
        (body["remaining"].as_str(), body["status"].as_str()),
        (Some("0"), Some("filled"))
    );
    assert_eq!(body["trades"].as_array().unwrap().len(), 1);

    let (_, orders) = get_json(&app, "/v1/me/orders", Some(&bo)).await;
    let orders = orders["orders"].as_array().unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0]["remaining"], "6");
    assert_eq!(orders[0]["side"], "Sell");
    let (_, ada_orders) = get_json(&app, "/v1/me/orders", Some(&ada)).await;
    assert!(ada_orders["orders"].as_array().unwrap().is_empty());

    let (_, account) = get_json(&app, "/v1/me/account", Some(&ada)).await;
    assert_eq!(account["display_name"], "Ada");
    assert_eq!(account["cash"], "-402.00");
    assert_eq!(account["positions"][0]["quantity"], "4");
    assert_eq!(account["pnl"], "0.00");
}

#[actix_web::test]
async fn market_reads_are_public() {
    let state = state();
    open_market(&state).await;
    let app = public_app(state.clone(), public_config(Key::generate())).await;
    let (ada, _) = join(&app, "Ada", 1).await;
    let (bo, _) = join(&app, "Bo", 2).await;
    for (cookie, side, qty) in [(&bo, "Sell", "5"), (&ada, "Buy", "5")] {
        test::call_service(
            &app,
            req(Method::POST, "/v1/orders")
                .cookie(cookie.clone())
                .set_json(limit_order(None, side, "101", qty))
                .to_request(),
        )
        .await;
    }

    let (status, symbols) = get_json(&app, "/v1/symbols", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(symbols[0]["symbol"], SYM);
    assert_eq!(symbols[0]["state"], "Open");
    assert_eq!(symbols[0]["last"], "101");
    assert_eq!(
        (
            symbols[0]["collar_low"].as_str(),
            symbols[0]["collar_high"].as_str()
        ),
        (Some("80"), Some("120"))
    );

    let (_, trades) = get_json(&app, &format!("/v1/trades/{SYM}?limit=1"), None).await;
    assert_eq!(trades["trades"].as_array().unwrap().len(), 1);
    assert_eq!(trades["trades"][0]["price"], "101");
    assert_eq!(
        get_json(&app, "/v1/trades/NOPE", None).await.0,
        StatusCode::NOT_FOUND
    );

    let (_, board) = get_json(&app, "/v1/leaderboard", Some(&ada)).await;
    let entries = board["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let you: Vec<(&str, bool)> = entries
        .iter()
        .map(|e| {
            (
                e["display_name"].as_str().unwrap(),
                e["you"].as_bool().unwrap(),
            )
        })
        .collect();
    assert!(you.contains(&("Ada", true)) && you.contains(&("Bo", false)));
    assert!(
        entries.iter().all(|e| e.get("client_id").is_none()),
        "client ids stay private"
    );
}

#[actix_web::test]
async fn market_reset_is_admin_only() {
    let state = state();
    let public = public_app(state.clone(), public_config(Key::generate())).await;
    let admin = admin_app(state.clone()).await;
    let resp = test::call_service(&public, req(Method::POST, "/v1/admin/reset").to_request()).await;
    assert!(
        matches!(
            resp.status(),
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ),
        "{}",
        resp.status()
    );

    open_market(&state).await;
    let resp = test::call_service(
        &admin,
        test::TestRequest::post()
            .uri("/v1/admin/reset")
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body, json!({ "guests_kept": 0, "guests_removed": 0 }));
    assert_eq!(
        state.symbol_summaries().await[0].state,
        engine::session::SessionState::PreMarket
    );
}

#[actix_web::test]
async fn stream_sends_order_events_only_to_their_owner() {
    use market_data::MarketEvent;
    use server::handlers::ws::forwarded;
    let private = MarketEvent::OrderCancelled {
        order_id: common::OrderId::new(),
        symbol: Symbol::new(SYM),
        client_id: "g-ada".into(),
        side: Side::Buy,
        remaining: dec!(1),
    };
    assert!(forwarded(&private, Some("g-ada"), false));
    assert!(!forwarded(&private, Some("g-bo"), false));
    assert!(!forwarded(&private, None, false));
    assert!(
        forwarded(&private, None, true),
        "admin stream sees everything"
    );
    assert!(forwarded(&MarketEvent::MarketReset, None, false));
}
