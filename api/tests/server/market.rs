//! Market events, read-side queries and the market reset, driven through `AppState`.

use chrono::{Duration, Utc};
use common::Symbol;
use engine::order::{Order, OrderStatus, Side, TimeInForce};
use engine::session::SessionState;
use market_data::{EventBus, MarketEvent};
use persistence::wal::{WalEntry, WriteAheadLog};
use risk::{ParticipantLimits, RiskEngine, SymbolLimits};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use server::exchange::Participant;
use server::state::{AppState, ParticipantKind};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::broadcast;

const SYM: &str = "AAPL";
const OTHER: &str = "MSFT";

fn state() -> AppState {
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
    let symbol_limits = [SYM, OTHER]
        .into_iter()
        .map(|s| {
            (
                Symbol::new(s),
                SymbolLimits {
                    active: true,
                    price_collar_pct: dec!(20),
                    reference_price: dec!(100),
                },
            )
        })
        .collect();
    AppState::new(
        &[SYM.to_string(), OTHER.to_string()],
        RiskEngine::new(participant_limits, symbol_limits),
        Arc::new(EventBus::new(256)),
        None,
    )
}

async fn opened() -> AppState {
    let s = state();
    s.transition(SYM, |t| t.open()).await.unwrap();
    s.transition(OTHER, |t| t.open()).await.unwrap();
    s
}

fn limit(symbol: &str, side: Side, price: Decimal, qty: Decimal, client: &str) -> Order {
    Order::new_limit(
        Symbol::new(symbol),
        side,
        price,
        qty,
        TimeInForce::GoodTillCanceled,
        client,
    )
}

fn ioc(side: Side, price: Decimal, qty: Decimal, client: &str) -> Order {
    Order::new_limit(
        Symbol::new(SYM),
        side,
        price,
        qty,
        TimeInForce::ImmediateOrCancel,
        client,
    )
}

fn guest(s: &AppState, name: &str) -> Participant {
    s.register_participant(name.to_string(), dec!(100_000), dec!(10_000), 100)
        .unwrap()
}

fn drain(rx: &mut broadcast::Receiver<MarketEvent>) -> Vec<MarketEvent> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

fn tag(event: &MarketEvent) -> &'static str {
    match event {
        MarketEvent::Trade(_) => "trade",
        MarketEvent::OrderAccepted { .. } => "order_accepted",
        MarketEvent::OrderCancelled { .. } => "order_cancelled",
        MarketEvent::SessionState { .. } => "session_state",
        MarketEvent::BookUpdate { .. } => "book_update",
        MarketEvent::SandboxRemoved { .. } => "sandbox_removed",
        MarketEvent::MarketReset => "market_reset",
    }
}

#[tokio::test]
async fn submit_publishes_trades_then_a_private_accepted_event() {
    let s = opened().await;
    s.submit(limit(SYM, Side::Sell, dec!(100), dec!(4), "bob"))
        .await
        .unwrap();
    let mut rx = s.bus.subscribe();

    let buy = limit(SYM, Side::Buy, dec!(100), dec!(10), "alice");
    s.submit(buy.clone()).await.unwrap();

    let events = drain(&mut rx);
    assert_eq!(
        events.iter().map(tag).collect::<Vec<_>>(),
        ["trade", "order_accepted"]
    );
    match &events[1] {
        MarketEvent::OrderAccepted {
            order_id,
            client_id,
            quantity,
            remaining,
            status,
            ..
        } => {
            assert_eq!(*order_id, buy.id);
            assert_eq!(client_id, "alice");
            assert_eq!((*quantity, *remaining), (dec!(10), dec!(6)));
            assert_eq!(*status, OrderStatus::Resting);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(events[1].owner(), Some("alice"));
    assert_eq!(events[0].owner(), None);
}

#[tokio::test]
async fn cancel_publishes_the_remaining_quantity_to_the_owner() {
    let s = opened().await;
    let resting = limit(SYM, Side::Sell, dec!(101), dec!(10), "bob");
    s.submit(resting.clone()).await.unwrap();
    s.submit(ioc(Side::Buy, dec!(101), dec!(3), "alice"))
        .await
        .unwrap();
    let mut rx = s.bus.subscribe();

    s.cancel(resting.id, SYM, Side::Sell).await.unwrap();
    match drain(&mut rx).as_slice() {
        [
            MarketEvent::OrderCancelled {
                client_id,
                remaining,
                ..
            },
        ] => {
            assert_eq!(client_id, "bob");
            assert_eq!(*remaining, dec!(7));
        }
        other => panic!("unexpected {other:?}"),
    }

    // Cancelling it again finds nothing and publishes nothing.
    s.cancel(resting.id, SYM, Side::Sell).await.unwrap();
    assert!(drain(&mut rx).is_empty());
}

#[tokio::test]
async fn book_updates_are_coalesced_per_symbol() {
    let s = opened().await;
    s.publish_dirty_books().await;
    let mut rx = s.bus.subscribe();

    s.submit(limit(SYM, Side::Buy, dec!(99), dec!(5), "alice"))
        .await
        .unwrap();
    s.submit(limit(SYM, Side::Buy, dec!(99), dec!(5), "bob"))
        .await
        .unwrap();
    drain(&mut rx);

    s.publish_dirty_books().await;
    match drain(&mut rx).as_slice() {
        [MarketEvent::BookUpdate { symbol, bids, asks }] => {
            assert_eq!(symbol.as_str(), SYM);
            assert_eq!(bids, &vec![(dec!(99), dec!(10))]);
            assert!(asks.is_empty());
        }
        other => panic!("unexpected {other:?}"),
    }

    s.publish_dirty_books().await;
    assert!(drain(&mut rx).is_empty(), "nothing changed, nothing sent");
}

#[tokio::test]
async fn accounts_and_leaderboard_mark_positions_to_the_last_trade() {
    let s = opened().await;
    let ada = guest(&s, "Ada");
    let bo = guest(&s, "Bo");
    s.submit(limit(SYM, Side::Sell, dec!(100), dec!(10), &bo.client_id))
        .await
        .unwrap();
    s.submit(ioc(Side::Buy, dec!(100), dec!(10), &ada.client_id))
        .await
        .unwrap();
    // A later trade between others moves the mark to 102.
    s.submit(limit(SYM, Side::Sell, dec!(102), dec!(1), "alice"))
        .await
        .unwrap();
    s.submit(ioc(Side::Buy, dec!(102), dec!(1), "bob"))
        .await
        .unwrap();

    let account = s.account(&ada.client_id).unwrap();
    assert_eq!(account.cash, dec!(-1000));
    assert_eq!(account.positions.len(), 1);
    assert_eq!(account.positions[0].quantity, dec!(10));
    assert_eq!(account.positions[0].mark, dec!(102));
    assert_eq!(account.pnl, dec!(20));
    assert_eq!(s.account(&bo.client_id).unwrap().pnl, dec!(-20));

    // alice and bob are unnamed participants, so only the two guests are ranked.
    let board = s.leaderboard(10, Some(&bo.client_id));
    let rows: Vec<_> = board
        .iter()
        .map(|e| (e.rank, e.display_name.as_str(), e.pnl, e.you))
        .collect();
    assert_eq!(
        rows,
        [(1, "Ada", dec!(20), false), (2, "Bo", dec!(-20), true)]
    );
    assert!(board.iter().all(|e| e.kind == ParticipantKind::Guest));
}

#[tokio::test]
async fn symbol_summaries_report_session_stats_and_collar() {
    let s = state();
    s.transition(SYM, |t| t.open()).await.unwrap();
    for (price, qty) in [(dec!(101), dec!(2)), (dec!(99), dec!(3))] {
        s.submit(limit(SYM, Side::Sell, price, qty, "bob"))
            .await
            .unwrap();
        s.submit(ioc(Side::Buy, price, qty, "alice")).await.unwrap();
    }

    let summaries = s.symbol_summaries(server::sandbox::Viewer::Admin).await;
    assert_eq!(
        summaries
            .iter()
            .map(|x| x.symbol.as_str())
            .collect::<Vec<_>>(),
        [SYM, OTHER]
    );
    let aapl = &summaries[0];
    assert_eq!(aapl.state, SessionState::Open);
    assert_eq!(
        (aapl.open, aapl.last, aapl.high, aapl.low),
        (
            Some(dec!(101)),
            Some(dec!(99)),
            Some(dec!(101)),
            Some(dec!(99))
        )
    );
    assert_eq!((aapl.volume, aapl.trades), (dec!(5), 2));
    assert_eq!((aapl.collar_low, aapl.collar_high), (dec!(80), dec!(120)));
    assert_eq!(summaries[1].state, SessionState::PreMarket);
    assert_eq!(summaries[1].last, None);
}

#[tokio::test]
async fn trade_history_is_newest_first_and_live_only() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();
    let live = opened().await;
    *live.wal.lock().unwrap() = Some(WriteAheadLog::open(path).unwrap());
    for price in [dec!(100), dec!(101)] {
        live.submit(limit(SYM, Side::Sell, price, dec!(1), "bob"))
            .await
            .unwrap();
        live.submit(ioc(Side::Buy, price, dec!(1), "alice"))
            .await
            .unwrap();
    }
    let prices: Vec<Decimal> = live
        .recent_trades(SYM, 10)
        .unwrap()
        .iter()
        .map(|t| t.price)
        .collect();
    assert_eq!(prices, [dec!(101), dec!(100)]);
    assert_eq!(live.recent_trades(SYM, 1).unwrap().len(), 1);
    assert!(live.recent_trades("NOPE", 10).is_none());

    // Replay restores prices and volume, but not the trade list: replayed trades get new
    // ids and timestamps.
    let recovered = state();
    let mut entries = WriteAheadLog::replay(path).unwrap();
    entries.insert(
        0,
        WalEntry::SessionStateChanged {
            symbol: SYM.into(),
            state: SessionState::Open,
        },
    );
    recovered.replay(entries).await.unwrap();
    assert!(recovered.recent_trades(SYM, 10).unwrap().is_empty());
    let summary = &recovered.symbol_summaries(server::sandbox::Viewer::Admin).await[0];
    assert_eq!((summary.last, summary.volume), (Some(dec!(101)), dec!(2)));
}

#[tokio::test]
async fn reset_clears_the_market_and_keeps_recent_guests() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();
    let live = opened().await;
    *live.wal.lock().unwrap() = Some(WriteAheadLog::open(path).unwrap());

    // An old guest, as if replayed from a WAL written two days ago.
    live.replay(vec![WalEntry::ParticipantRegistered {
        client_id: "g-oldoldold0".into(),
        display_name: "Old".into(),
        max_notional_per_order: dec!(100_000),
        max_position: dec!(10_000),
        max_orders_per_second: 5,
        registered_at: Some(Utc::now() - Duration::days(2)),
    }])
    .await
    .unwrap();
    let recent = guest(&live, "Recent");
    live.submit(limit(SYM, Side::Sell, dec!(100), dec!(5), "bob"))
        .await
        .unwrap();
    live.submit(ioc(Side::Buy, dec!(100), dec!(2), &recent.client_id))
        .await
        .unwrap();
    live.submit(limit(OTHER, Side::Buy, dec!(90), dec!(1), "alice"))
        .await
        .unwrap();
    let mut rx = live.bus.subscribe();

    let summary = live
        .reset_market(Utc::now() - Duration::hours(24))
        .await
        .unwrap();
    assert_eq!((summary.guests_kept, summary.guests_removed), (1, 1));

    assert!(live.open_orders(None).await.is_empty());
    for s in live.symbol_summaries(server::sandbox::Viewer::Admin).await {
        assert_eq!(s.state, SessionState::PreMarket);
        assert_eq!((s.last, s.volume), (None, Decimal::ZERO));
    }
    assert!(live.recent_trades(SYM, 10).unwrap().is_empty());
    let account = live.account(&recent.client_id).unwrap();
    assert!(account.positions.is_empty());
    assert_eq!(account.pnl, Decimal::ZERO);
    assert!(live.participant("g-oldoldold0").is_none());
    assert!(
        !live
            .risk
            .lock()
            .unwrap()
            .participant_limits
            .contains_key("g-oldoldold0")
    );

    let tags: Vec<_> = drain(&mut rx).iter().map(tag).collect();
    assert_eq!(tags, ["market_reset", "session_state", "session_state"]);

    // The new WAL holds only the kept guest, so a restart restores them and nothing else.
    let entries = WriteAheadLog::replay(path).unwrap();
    assert_eq!(entries.len(), 1);
    let restarted = state();
    restarted.replay(entries).await.unwrap();
    assert_eq!(
        restarted.participant(&recent.client_id),
        Some(recent.clone())
    );
    assert!(restarted.open_orders(None).await.is_empty());
    assert_eq!(
        WriteAheadLog::replay(&format!("{path}.prev"))
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn next_reset_is_later_today_or_tomorrow() {
    use chrono::{NaiveTime, TimeZone};
    use server::reset::{next_occurrence, parse_reset_at};
    let at = NaiveTime::from_hms_opt(4, 0, 0).unwrap();
    let before = Utc.with_ymd_and_hms(2026, 10, 13, 3, 59, 0).unwrap();
    let after = Utc.with_ymd_and_hms(2026, 10, 13, 4, 0, 0).unwrap();
    assert_eq!(
        next_occurrence(before, at),
        Utc.with_ymd_and_hms(2026, 10, 13, 4, 0, 0).unwrap()
    );
    assert_eq!(
        next_occurrence(after, at),
        Utc.with_ymd_and_hms(2026, 10, 14, 4, 0, 0).unwrap()
    );
    assert_eq!(parse_reset_at(" 04:00 ").unwrap(), at);
    assert!(parse_reset_at("4am").is_err());
    assert!(parse_reset_at("25:00").is_err());
}
