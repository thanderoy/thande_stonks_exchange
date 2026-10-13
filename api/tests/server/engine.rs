//! Engine view, activity and auction events, metrics and load bursts.

use actix_web::cookie::{Cookie, Key};
use actix_web::http::{Method, StatusCode};
use actix_web::{App, test, web};
use common::Symbol;
use engine::order::{Order, Side, TimeInForce};
use market_data::{Activity, EventBus, MarketEvent};
use risk::{ParticipantLimits, RiskEngine, SymbolLimits};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{Value, json};
use server::app::{PublicConfig, PublicLimits, SESSION_COOKIE, configure_admin, configure_public};
use server::bots;
use server::burst::LOAD_ID;
use server::exchange::ExchangeError;
use server::metrics::Metrics;
use server::state::AppState;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

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
                    max_position: dec!(100_000),
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
    let s = AppState::new(
        &[SYM.to_string()],
        RiskEngine::new(participant_limits, symbol_limits),
        Arc::new(EventBus::new(4096)),
        None,
    );
    bots::register(&s);
    Arc::new(s)
}

fn order(side: Side, price: Decimal, qty: Decimal, tif: TimeInForce, who: &str) -> Order {
    Order::new_limit(Symbol::new(SYM), side, price, qty, tif, who)
}

fn activities(rx: &mut tokio::sync::broadcast::Receiver<MarketEvent>) -> Vec<(Activity, usize)> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|e| match e {
            MarketEvent::OrderActivity { action, fills, .. } => Some((action, fills)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn activity_events_say_what_happened_to_each_order() {
    let s = state();
    s.transition(SYM, |t| t.open()).await.unwrap();
    let gtc = TimeInForce::GoodTillCanceled;
    s.submit(order(Side::Sell, dec!(100), dec!(5), gtc.clone(), "bob"))
        .await
        .unwrap();
    let mut rx = s.bus.subscribe();

    // FOK that can't fill → killed; IOC partly filled → expired; then filled; then rested.
    s.submit(order(
        Side::Buy,
        dec!(100),
        dec!(9),
        TimeInForce::FillOrKill,
        "alice",
    ))
    .await
    .unwrap();
    s.submit(order(
        Side::Buy,
        dec!(100),
        dec!(3),
        TimeInForce::ImmediateOrCancel,
        "alice",
    ))
    .await
    .unwrap();
    s.submit(order(
        Side::Buy,
        dec!(100),
        dec!(2),
        TimeInForce::ImmediateOrCancel,
        "alice",
    ))
    .await
    .unwrap();
    s.submit(order(
        Side::Buy,
        dec!(100),
        dec!(1),
        TimeInForce::ImmediateOrCancel,
        "alice",
    ))
    .await
    .unwrap();
    let resting = order(Side::Buy, dec!(99), dec!(4), gtc, "alice");
    s.submit(resting.clone()).await.unwrap();
    s.cancel(resting.id, SYM, Side::Buy).await.unwrap();

    assert_eq!(
        activities(&mut rx),
        [
            (Activity::Killed, 0),
            (Activity::Filled, 1),
            (Activity::Filled, 1),
            (Activity::Expired, 0),
            (Activity::Rested, 0),
            (Activity::Cancelled, 0),
        ]
    );
}

#[tokio::test]
async fn opening_publishes_one_auction_summary() {
    let s = state();
    let gtc = TimeInForce::GoodTillCanceled;
    s.submit(order(Side::Buy, dec!(101), dec!(10), gtc.clone(), "alice"))
        .await
        .unwrap();
    s.submit(order(Side::Sell, dec!(99), dec!(6), gtc, "bob"))
        .await
        .unwrap();
    let mut rx = s.bus.subscribe();
    s.transition(SYM, |t| t.open()).await.unwrap();

    let auctions: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|e| match e {
            MarketEvent::Auction { volume, trades, .. } => Some((volume, trades)),
            _ => None,
        })
        .collect();
    assert_eq!(auctions, [(dec!(6), 1)]);
}

#[::core::prelude::v1::test]
fn metrics_keep_a_minute_of_per_second_stats() {
    let m = Metrics::default();
    let t = 1_000_000;
    for us in [100, 200, 300, 400] {
        m.record_submit_at(t, Duration::from_micros(us), true);
    }
    m.record_submit_at(t + 1, Duration::from_micros(1000), false);
    m.record_submit_at(t - 120, Duration::from_micros(5), true); // outside the window

    let stats = m.stats_at(t + 1);
    assert_eq!(stats.seconds.len(), 60);
    let last = &stats.seconds[59];
    assert_eq!((last.t, last.orders, last.rejected), (t + 1, 1, 1));
    let prev = &stats.seconds[58];
    assert_eq!(
        (prev.orders, prev.p50_us, prev.p99_us),
        (4, Some(200), Some(400))
    );
    assert_eq!(stats.seconds[0].p50_us, None);
    assert_eq!((stats.orders, stats.rejected), (6, 1));
    assert_eq!(stats.p99_us, Some(1000));

    let text = m.prometheus();
    assert!(text.contains("tse_orders_total 6"));
    assert!(text.contains("tse_orders_rejected_total 1"));
    assert!(text.contains("tse_submit_latency_seconds_bucket{le=\"0.0001\"} 2"));
    assert!(text.contains("tse_submit_latency_seconds_count 6"));
}

#[tokio::test]
async fn burst_fires_orders_and_withdraws_what_rests() {
    let s = state();
    assert!(matches!(
        s.burst(SYM, 10).await,
        Err(ExchangeError::NotAcceptingOrders(..))
    ));
    s.transition(SYM, |t| t.open()).await.unwrap();
    let before = s.metrics.stats().orders;

    let result = s.burst(SYM, 150).await.unwrap();
    assert_eq!(result.submitted, 150);
    assert_eq!(result.accepted + result.rejected, 150);
    assert!(result.accepted > 140, "{result:?}");
    assert!(result.trades > 0, "random orders around one price cross");
    assert!(result.p50_us <= result.p99_us);
    assert!(s.open_orders_in(SYM, Some(LOAD_ID)).await.is_empty());
    assert!(s.metrics.stats().orders >= before + 150);
}

#[tokio::test]
async fn guests_burst_only_their_open_sandbox_once_per_ten_seconds() {
    let s = state();
    assert_eq!(s.take_burst_slot("g-a"), 0);
    let wait = s.take_burst_slot("g-a");
    assert!((1..=10).contains(&wait), "{wait}");
    assert_eq!(s.take_burst_slot("g-b"), 0, "per guest");
}

fn peer(n: u8) -> SocketAddr {
    SocketAddr::from(([203, 0, 113, n], 40000))
}

#[actix_web::test]
async fn engine_view_and_bursts_over_http() {
    let state = state();
    state.transition(SYM, |t| t.open()).await.unwrap();
    let config = PublicConfig::new(Key::generate(), true, vec![], PublicLimits::default());
    let public = test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(move |cfg| configure_public(cfg, &config)),
    )
    .await;
    let admin = test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(configure_admin),
    )
    .await;

    let join = async |name: &str, ip: u8| -> Cookie<'static> {
        let resp = test::call_service(
            &public,
            test::TestRequest::post()
                .uri("/v1/join")
                .peer_addr(peer(ip))
                .set_json(json!({ "display_name": name }))
                .to_request(),
        )
        .await;
        resp.response()
            .cookies()
            .find(|c| c.name() == SESSION_COOKIE)
            .unwrap()
            .into_owned()
    };
    let call = async |method: Method, uri: &str, cookie: &Cookie<'static>, body: Option<Value>| {
        let mut req = test::TestRequest::default()
            .method(method)
            .uri(uri)
            .peer_addr(peer(9))
            .cookie(cookie.clone());
        if let Some(body) = body {
            req = req.set_json(body);
        }
        let resp = test::call_service(&public, req.to_request()).await;
        let status = resp.status();
        let retry = resp
            .headers()
            .get("Retry-After")
            .map(|v| v.to_str().unwrap().to_string());
        let bytes = test::read_body(resp).await;
        (
            status,
            serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            retry,
        )
    };
    let ada = join("Ada", 1).await;
    let bo = join("Bo", 2).await;

    for (cookie, qty) in [(&ada, "10"), (&bo, "20")] {
        let body = json!({
            "symbol": SYM, "side": "Buy", "order_type": { "type": "Limit", "price": "99.00" },
            "quantity": qty, "time_in_force": "GoodTillCanceled",
        });
        assert_eq!(
            call(Method::POST, "/v1/orders", cookie, Some(body)).await.0,
            StatusCode::OK
        );
    }
    let (status, view, _) = call(Method::GET, &format!("/v1/engine/{SYM}"), &bo, None).await;
    assert_eq!(status, StatusCode::OK);
    let queue = &view["bids"][0];
    assert_eq!(queue["price"], "99.00");
    let rows: Vec<(String, bool)> = queue["orders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["participant"].as_str().unwrap().to_string(),
                o["you"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [("Ada".to_string(), false), ("Bo".to_string(), true)],
        "FIFO order"
    );

    // Bursts need an open sandbox of your own, and are capped.
    let burst = |n: usize| Some(json!({ "count": n }));
    assert_eq!(
        call(Method::POST, "/v1/me/sandbox/burst", &ada, burst(201))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(Method::POST, "/v1/me/sandbox/burst", &ada, burst(50))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, created, _) = call(Method::POST, "/v1/me/sandbox", &ada, None).await;
    let sandbox = created["symbol"].as_str().unwrap().to_string();
    let (status, _, _) = call(Method::POST, "/v1/me/sandbox/burst", &ada, burst(50)).await;
    assert_eq!(status, StatusCode::CONFLICT, "pre-market");
    call(
        Method::POST,
        &format!("/v1/session/{sandbox}/open"),
        &ada,
        None,
    )
    .await;
    let (status, result, _) = call(Method::POST, "/v1/me/sandbox/burst", &ada, burst(50)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (result["symbol"].as_str(), result["submitted"].as_u64()),
        (Some(sandbox.as_str()), Some(50))
    );
    let (status, _, retry) = call(Method::POST, "/v1/me/sandbox/burst", &ada, burst(50)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(retry.is_some());
    // Bo can't look inside Ada's sandbox.
    assert_eq!(
        call(Method::GET, &format!("/v1/engine/{sandbox}"), &bo, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    let (status, stats, _) = call(Method::GET, "/v1/stats", &bo, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(stats["orders"].as_u64().unwrap() >= 52);
    assert_eq!(stats["seconds"].as_array().unwrap().len(), 60);

    // /metrics and admin bursts only on the admin listener.
    let resp = test::call_service(
        &public,
        test::TestRequest::get()
            .uri("/metrics")
            .peer_addr(peer(9))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = test::call_service(
        &admin,
        test::TestRequest::get().uri("/metrics").to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let text = String::from_utf8(test::read_body(resp).await.to_vec()).unwrap();
    assert!(text.contains("tse_wal_write_latency_seconds_count"));
    let resp = test::call_service(
        &admin,
        test::TestRequest::post()
            .uri("/v1/admin/burst")
            .set_json(json!({ "symbol": SYM, "count": 20 }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}
