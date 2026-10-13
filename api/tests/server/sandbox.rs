//! Sandboxes: creation, privacy, the auction demo, removal, replay and the HTTP routes.

use actix_web::cookie::{Cookie, Key};
use actix_web::http::{Method, StatusCode};
use actix_web::{App, test, web};
use chrono::Utc;
use common::Symbol;
use engine::order::{Order, Side, TimeInForce};
use engine::session::SessionState;
use market_data::{EventBus, MarketEvent};
use persistence::wal::{WalEntry, WriteAheadLog};
use risk::{RiskEngine, SymbolLimits};
use rust_decimal_macros::dec;
use serde_json::{Value, json};
use server::app::{PublicConfig, PublicLimits, SESSION_COOKIE, configure_public};
use server::bots;
use server::exchange::ExchangeError;
use server::handlers::ws::forwarded;
use server::sandbox::Viewer;
use server::state::AppState;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

const SHARED: &str = "AAPL";

fn state() -> AppState {
    let symbol_limits = HashMap::from([(
        Symbol::new(SHARED),
        SymbolLimits {
            active: true,
            price_collar_pct: dec!(20),
            reference_price: dec!(100),
        },
    )]);
    let s = AppState::new(
        &[SHARED.to_string()],
        RiskEngine::new(HashMap::new(), symbol_limits),
        Arc::new(EventBus::new(256)),
        None,
    );
    bots::register(&s);
    s
}

fn guest(s: &AppState, name: &str) -> String {
    s.register_participant(name.into(), dec!(100_000), dec!(10_000), 100)
        .unwrap()
        .client_id
}

fn gtc(
    symbol: &str,
    side: Side,
    price: rust_decimal::Decimal,
    qty: rust_decimal::Decimal,
    who: &str,
) -> Order {
    Order::new_limit(
        Symbol::new(symbol),
        side,
        price,
        qty,
        TimeInForce::GoodTillCanceled,
        who,
    )
}

async fn state_of(s: &AppState, symbol: &str) -> SessionState {
    s.sessions.get(symbol).unwrap().lock().await.state.clone()
}

#[tokio::test]
async fn one_private_pre_market_sandbox_per_guest() {
    let s = state();
    let ada = guest(&s, "Ada");
    let bo = guest(&s, "Bo");

    let sandbox = s.create_sandbox(&ada).unwrap();
    assert!(
        sandbox.starts_with("SBX-") && sandbox.len() == 10,
        "{sandbox}"
    );
    assert_eq!(
        s.create_sandbox(&ada).unwrap(),
        sandbox,
        "creating again returns it"
    );
    assert_eq!(s.sandbox_of(&ada), Some(sandbox.clone()));
    assert_eq!(state_of(&s, &sandbox).await, SessionState::PreMarket);

    assert!(s.visible_to(&sandbox, Viewer::Public(Some(&ada))));
    assert!(!s.visible_to(&sandbox, Viewer::Public(Some(&bo))));
    assert!(!s.visible_to(&sandbox, Viewer::Public(None)));
    assert!(s.visible_to(&sandbox, Viewer::Admin));
    assert!(s.visible_to(SHARED, Viewer::Public(None)));

    let for_bo: Vec<String> = s
        .symbol_summaries(Viewer::Public(Some(&bo)))
        .await
        .into_iter()
        .map(|x| x.symbol)
        .collect();
    assert_eq!(for_bo, [SHARED]);
    let for_ada = s.symbol_summaries(Viewer::Public(Some(&ada))).await;
    assert_eq!(for_ada.len(), 2);
    assert_eq!(for_ada[1].symbol, sandbox);
    assert_eq!(
        (for_ada[1].collar_low, for_ada[1].collar_high),
        (dec!(80), dec!(120))
    );
}

#[tokio::test]
async fn sandbox_count_is_capped() {
    let mut s = state();
    s.sandbox_config.max = 1;
    let ada = guest(&s, "Ada");
    let bo = guest(&s, "Bo");
    s.create_sandbox(&ada).unwrap();
    let err = s.create_sandbox(&bo).unwrap_err();
    assert!(matches!(err, ExchangeError::SandboxesFull(1)));
    assert_eq!(err.status(), 503);
}

#[tokio::test]
async fn seeded_sandbox_runs_an_auction_without_touching_pnl() {
    let s = state();
    let ada = guest(&s, "Ada");
    let sandbox = s.create_sandbox(&ada).unwrap();
    s.submit(gtc(&sandbox, Side::Buy, dec!(100.30), dec!(50), &ada))
        .await
        .unwrap();

    let seeded = s.seed_sandbox(&sandbox).await.unwrap();
    assert_eq!((seeded.orders, seeded.crossed), (6, true));
    let (state, trades) = s.transition(&sandbox, |t| t.open()).await.unwrap();
    assert_eq!(state, SessionState::Open);
    assert!(
        !trades.is_empty(),
        "the opening auction matched the crossed book"
    );
    assert!(
        trades.iter().all(|t| t.price == trades[0].price),
        "one auction price"
    );
    assert!(trades.iter().any(|t| t.buyer_client_id == ada));

    // Positions move (they count for limits), but cash and P&L ignore sandboxes.
    let held = s.risk.lock().unwrap().participant_limits[&ada]
        .current_position
        .get(&Symbol::new(&sandbox))
        .copied();
    assert_eq!(held, Some(dec!(50)));
    let account = s.account(&ada).unwrap();
    assert_eq!((account.cash, account.pnl), (dec!(0), dec!(0)));
    assert!(account.positions.is_empty());
    assert!(s.leaderboard(10, None).is_empty());

    // Once open, seeding builds a two-sided book instead.
    let seeded = s.seed_sandbox(&sandbox).await.unwrap();
    assert!(!seeded.crossed);
}

#[tokio::test]
async fn removing_a_sandbox_tells_only_its_owner_and_forgets_it() {
    let s = state();
    let ada = guest(&s, "Ada");
    let sandbox = s.create_sandbox(&ada).unwrap();
    s.submit(gtc(&sandbox, Side::Buy, dec!(99), dec!(5), &ada))
        .await
        .unwrap();
    let mut rx = s.bus.subscribe();

    assert!(s.remove_sandbox(&sandbox, "deleted").await.unwrap());
    assert!(!s.remove_sandbox(&sandbox, "deleted").await.unwrap());
    assert!(s.sandbox_of(&ada).is_none());
    assert!(!s.shards.contains_key(&sandbox));
    assert!(s.open_orders(Some(&ada)).await.is_empty());
    assert!(matches!(
        s.submit(gtc(&sandbox, Side::Buy, dec!(99), dec!(5), &ada))
            .await,
        Err(ExchangeError::UnknownSymbol(_))
    ));

    let event = rx.try_recv().unwrap();
    assert!(matches!(&event, MarketEvent::SandboxRemoved { reason, .. } if reason == "deleted"));
    assert_eq!(event.owner(), Some(ada.as_str()));
    // A new one gets a fresh name.
    assert_ne!(s.create_sandbox(&ada).unwrap(), sandbox);
}

#[tokio::test]
async fn idle_sandboxes_are_swept() {
    let mut s = state();
    s.sandbox_config.idle = Duration::from_millis(50);
    let ada = guest(&s, "Ada");
    let bo = guest(&s, "Bo");
    let idle = s.create_sandbox(&ada).unwrap();
    let busy = s.create_sandbox(&bo).unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    s.submit(gtc(&busy, Side::Buy, dec!(99), dec!(1), &bo))
        .await
        .unwrap();

    assert_eq!(s.sweep_idle_sandboxes().await, 1);
    assert!(!s.is_sandbox(&idle));
    assert!(s.is_sandbox(&busy));
}

#[tokio::test]
async fn sandboxes_replay_from_the_wal_and_resets_remove_them() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();
    let live = state();
    *live.wal.lock().unwrap() = Some(WriteAheadLog::open(path).unwrap());
    let ada = guest(&live, "Ada");
    let bo = guest(&live, "Bo");
    let kept = live.create_sandbox(&ada).unwrap();
    let gone = live.create_sandbox(&bo).unwrap();
    live.submit(gtc(&kept, Side::Sell, dec!(101), dec!(7), &ada))
        .await
        .unwrap();
    live.transition(&kept, |t| t.open()).await.unwrap();
    live.remove_sandbox(&gone, "deleted").await.unwrap();

    let restarted = state();
    let entries = WriteAheadLog::replay(path).unwrap();
    assert!(
        entries
            .iter()
            .any(|e| matches!(e, WalEntry::SymbolCreated { .. }))
    );
    let stats = restarted.replay(entries).await.unwrap();
    assert_eq!(stats.skipped, 0);
    assert_eq!(restarted.sandbox_of(&ada), Some(kept.clone()));
    assert!(restarted.sandbox_of(&bo).is_none());
    assert_eq!(state_of(&restarted, &kept).await, SessionState::Open);
    assert_eq!(restarted.open_orders(Some(&ada)).await.len(), 1);

    live.reset_market(Utc::now() - chrono::Duration::hours(24))
        .await
        .unwrap();
    assert!(live.sandboxes.is_empty());
    assert!(!live.shards.contains_key(&kept));
    // The new WAL has no sandbox in it.
    assert!(
        !WriteAheadLog::replay(path)
            .unwrap()
            .iter()
            .any(|e| matches!(e, WalEntry::SymbolCreated { .. }))
    );
}

#[::core::prelude::v1::test]
fn sandbox_events_reach_only_the_owner() {
    let trade_on_sandbox = MarketEvent::SessionState {
        symbol: Symbol::new("SBX-AAAAAA"),
        state: "Open".into(),
    };
    assert!(forwarded(
        &trade_on_sandbox,
        Some("g-ada"),
        false,
        Some("g-ada")
    ));
    assert!(!forwarded(
        &trade_on_sandbox,
        Some("g-bo"),
        false,
        Some("g-ada")
    ));
    assert!(!forwarded(&trade_on_sandbox, None, false, Some("g-ada")));
    assert!(forwarded(&trade_on_sandbox, None, true, Some("g-ada")));
    assert!(
        forwarded(&trade_on_sandbox, None, false, None),
        "shared symbols are public"
    );
}

// ── HTTP ────────────────────────────────────────────────────────────────────

fn peer(n: u8) -> SocketAddr {
    SocketAddr::from(([203, 0, 113, n], 40000))
}

#[actix_web::test]
async fn sandbox_routes_are_owner_only() {
    let state = Arc::new(state());
    let config = PublicConfig::new(Key::generate(), true, vec![], PublicLimits::default());
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(move |cfg| configure_public(cfg, &config)),
    )
    .await;

    let join = async |name: &str, ip: u8| -> Cookie<'static> {
        let resp = test::call_service(
            &app,
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
    let ada = join("Ada", 1).await;
    let bo = join("Bo", 2).await;

    let call = async |method: Method, uri: &str, cookie: &Cookie<'static>, body: Option<Value>| {
        let mut req = test::TestRequest::default()
            .method(method)
            .uri(uri)
            .peer_addr(peer(9))
            .cookie(cookie.clone());
        if let Some(body) = body {
            req = req.set_json(body);
        }
        let resp = test::call_service(&app, req.to_request()).await;
        let status = resp.status();
        let bytes = test::read_body(resp).await;
        (
            status,
            serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
        )
    };

    assert_eq!(
        call(Method::GET, "/v1/me/sandbox", &ada, None).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, created) = call(Method::POST, "/v1/me/sandbox", &ada, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["state"], "PreMarket");
    let sym = created["symbol"].as_str().unwrap().to_string();

    // Bo can't see, trade or run it; the shared symbol stays admin-only for both.
    for uri in [
        format!("/v1/book/{sym}"),
        format!("/v1/session/{sym}"),
        format!("/v1/trades/{sym}"),
    ] {
        assert_eq!(
            call(Method::GET, &uri, &bo, None).await.0,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
        assert_eq!(
            call(Method::GET, &uri, &ada, None).await.0,
            StatusCode::OK,
            "{uri}"
        );
    }
    let order = json!({
        "symbol": sym, "side": "Buy", "order_type": { "type": "Limit", "price": "100.20" },
        "quantity": "10", "time_in_force": "GoodTillCanceled",
    });
    assert_eq!(
        call(Method::POST, "/v1/orders", &bo, Some(order.clone()))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(Method::POST, "/v1/orders", &ada, Some(order)).await.0,
        StatusCode::OK
    );
    let (_, symbols) = call(Method::GET, "/v1/symbols", &bo, None).await;
    assert_eq!(symbols.as_array().unwrap().len(), 1);
    let (_, symbols) = call(Method::GET, "/v1/symbols", &ada, None).await;
    assert_eq!(symbols[1]["kind"], "sandbox");

    let open = format!("/v1/session/{sym}/open");
    assert_eq!(
        call(Method::POST, &open, &bo, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            Method::POST,
            &format!("/v1/session/{SHARED}/open"),
            &ada,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let (status, seeded) = call(Method::POST, "/v1/me/sandbox/seed", &ada, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seeded["crossed"], true);
    let (status, opened) = call(Method::POST, &open, &ada, None).await;
    assert_eq!(
        (status, opened["state"].as_str()),
        (StatusCode::OK, Some("Open"))
    );
    let (_, trades) = call(Method::GET, &format!("/v1/trades/{sym}"), &ada, None).await;
    assert!(
        !trades["trades"].as_array().unwrap().is_empty(),
        "auction trades"
    );

    let halt = format!("/v1/session/{sym}/halt");
    let (status, halted) = call(Method::POST, &halt, &ada, Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(halted["state"].get("Halted").is_some());

    assert_eq!(
        call(Method::DELETE, "/v1/me/sandbox", &ada, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(Method::GET, "/v1/me/sandbox", &ada, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(state.sandboxes.is_empty());
}
