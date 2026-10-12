use common::Symbol;
use engine::order::{Order, Side, TimeInForce};
use engine::session::{HaltReason, SessionState};
use engine::shard::{BookSnapshot, EngineCommand};
use market_data::EventBus;
use persistence::wal::{WalEntry, WriteAheadLog};
use risk::{ParticipantLimits, RiskEngine, RiskError, SymbolLimits};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use server::exchange::{ExchangeError, Participant};
use server::state::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::mpsc;

const SYM: &str = "AAPL";

fn risk() -> RiskEngine {
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
    RiskEngine::new(participant_limits, symbol_limits)
}

fn state() -> AppState {
    AppState::new(
        &[SYM.to_string()],
        risk(),
        Arc::new(EventBus::new(64)),
        None,
    )
}

fn limit(side: Side, price: Decimal, qty: Decimal, client: &str) -> Order {
    Order::new_limit(
        Symbol::new(SYM),
        side,
        price,
        qty,
        TimeInForce::GoodTillCanceled,
        client,
    )
}

async fn book(state: &AppState) -> BookSnapshot {
    let shard = state.shards.get(SYM).unwrap().clone();
    let (tx, mut rx) = mpsc::channel(1);
    shard.send(EngineCommand::GetBook(tx)).await.unwrap();
    rx.recv().await.unwrap()
}

fn position(state: &AppState, client: &str) -> Decimal {
    state.risk.lock().unwrap().participant_limits[client]
        .current_position
        .get(&Symbol::new(SYM))
        .copied()
        .unwrap_or(Decimal::ZERO)
}

async fn session_state(state: &AppState) -> SessionState {
    state.sessions.get(SYM).unwrap().lock().await.state.clone()
}

#[tokio::test]
async fn pre_market_rests_crossing_orders_without_matching() {
    let s = state();
    assert!(
        s.submit(limit(Side::Buy, dec!(101), dec!(10), "alice"))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        s.submit(limit(Side::Sell, dec!(99), dec!(10), "bob"))
            .await
            .unwrap()
            .is_empty()
    );
    let b = book(&s).await;
    assert_eq!(b.bids, vec![(dec!(101), dec!(10))]);
    assert_eq!(b.asks, vec![(dec!(99), dec!(10))]);
}

#[tokio::test]
async fn pre_market_rejects_orders_that_cannot_rest() {
    let s = state();
    let market = Order::new_market(Symbol::new(SYM), Side::Buy, dec!(1), "alice");
    let ioc = Order::new_limit(
        Symbol::new(SYM),
        Side::Buy,
        dec!(100),
        dec!(1),
        TimeInForce::ImmediateOrCancel,
        "alice",
    );
    for order in [market, ioc] {
        assert!(matches!(
            s.submit(order).await,
            Err(ExchangeError::PreOpenRequiresRestingLimit)
        ));
    }
}

#[tokio::test]
async fn open_runs_opening_auction_and_records_positions() {
    let s = state();
    s.submit(limit(Side::Buy, dec!(101), dec!(10), "alice"))
        .await
        .unwrap();
    s.submit(limit(Side::Sell, dec!(99), dec!(6), "bob"))
        .await
        .unwrap();

    let (state_after, trades) = s.transition(SYM, |t| t.open()).await.unwrap();

    assert_eq!(state_after, SessionState::Open);
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].quantity, dec!(6));
    let b = book(&s).await;
    assert_eq!(b.bids, vec![(dec!(101), dec!(4))]);
    assert!(b.asks.is_empty());
    assert_eq!(position(&s, "alice"), dec!(6));
    assert_eq!(position(&s, "bob"), dec!(-6));
}

#[tokio::test]
async fn halted_session_rejects_orders_but_allows_cancels() {
    let s = state();
    let resting = limit(Side::Buy, dec!(100), dec!(5), "alice");
    let resting_id = resting.id;
    s.submit(resting).await.unwrap();
    s.transition(SYM, |t| t.halt(HaltReason::TechnicalIssue))
        .await
        .unwrap();

    let err = s
        .submit(limit(Side::Buy, dec!(100), dec!(5), "alice"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ExchangeError::NotAcceptingOrders(_, SessionState::Halted { .. })
    ));
    assert_eq!(err.status(), 409);

    assert!(
        s.cancel(resting_id, SYM, Side::Buy)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn continuous_fills_update_positions_and_enforce_limit() {
    let s = state();
    s.transition(SYM, |t| t.open()).await.unwrap();
    s.submit(limit(Side::Sell, dec!(100), dec!(900), "bob"))
        .await
        .unwrap();
    s.submit(limit(Side::Buy, dec!(100), dec!(900), "alice"))
        .await
        .unwrap();
    assert_eq!(position(&s, "alice"), dec!(900));

    let err = s
        .submit(limit(Side::Buy, dec!(100), dec!(200), "alice"))
        .await
        .unwrap_err();
    assert!(matches!(err, ExchangeError::Risk(_)));
}

#[tokio::test]
async fn replaying_the_wal_rebuilds_book_positions_and_session() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();

    let live = state();
    *live.wal.lock().unwrap() = Some(WriteAheadLog::open(path).unwrap());
    live.submit(limit(Side::Buy, dec!(101), dec!(10), "alice"))
        .await
        .unwrap();
    live.submit(limit(Side::Sell, dec!(99), dec!(4), "bob"))
        .await
        .unwrap();
    let cancelled = limit(Side::Sell, dec!(110), dec!(7), "bob");
    let cancelled_id = cancelled.id;
    live.submit(cancelled).await.unwrap();
    live.transition(SYM, |t| t.open()).await.unwrap();
    live.cancel(cancelled_id, SYM, Side::Sell).await.unwrap();
    live.submit(limit(Side::Sell, dec!(101), dec!(3), "bob"))
        .await
        .unwrap();
    live.transition(SYM, |t| t.halt(HaltReason::RegulatoryHalt))
        .await
        .unwrap();

    let (_, entries) = WriteAheadLog::open_for_recovery(path).unwrap();
    let recovered = state();
    let stats = recovered.replay(entries).await.unwrap();

    assert_eq!(stats.skipped, 0);
    let (live_book, recovered_book) = (book(&live).await, book(&recovered).await);
    assert_eq!(recovered_book.bids, live_book.bids);
    assert_eq!(recovered_book.asks, live_book.asks);
    assert_eq!(recovered_book.bids, vec![(dec!(101), dec!(3))]);
    for client in ["alice", "bob"] {
        assert_eq!(position(&recovered, client), position(&live, client));
    }
    assert_eq!(position(&recovered, "alice"), dec!(7));
    assert_eq!(session_state(&recovered).await, session_state(&live).await);
}

#[tokio::test]
async fn replay_skips_symbols_not_configured() {
    let other = AppState::new(
        &["MSFT".to_string()],
        risk(),
        Arc::new(EventBus::new(8)),
        None,
    );
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();
    {
        let live = state();
        *live.wal.lock().unwrap() = Some(WriteAheadLog::open(path).unwrap());
        live.submit(limit(Side::Buy, dec!(100), dec!(1), "alice"))
            .await
            .unwrap();
    }
    let stats = other
        .replay(WriteAheadLog::replay(path).unwrap())
        .await
        .unwrap();
    assert_eq!((stats.applied, stats.skipped), (0, 1));
}

fn register_guest(state: &AppState, name: &str) -> Participant {
    state
        .register_participant(name.to_string(), dec!(100_000), dec!(10_000), 5)
        .unwrap()
}

#[tokio::test]
async fn registered_guest_can_trade_and_has_limits() {
    let s = state();
    let guest = register_guest(&s, "Guest One");
    assert!(guest.client_id.starts_with("g-"));
    assert_eq!(guest.client_id.len(), 12);
    assert_eq!(s.participant(&guest.client_id), Some(guest.clone()));
    s.submit(limit(Side::Buy, dec!(100), dec!(5), &guest.client_id))
        .await
        .unwrap();
    // 100 * 1001 = 100_100 > the guest's 100_000 notional limit.
    assert!(matches!(
        s.submit(limit(Side::Buy, dec!(100), dec!(1001), &guest.client_id))
            .await,
        Err(ExchangeError::Risk(RiskError::NotionalLimitExceeded))
    ));
}

#[tokio::test]
async fn order_rate_limit_rejects_with_429() {
    let s = state();
    let guest = register_guest(&s, "Speedy");
    for _ in 0..5 {
        s.submit(limit(Side::Buy, dec!(100), dec!(1), &guest.client_id))
            .await
            .unwrap();
    }
    let err = s
        .submit(limit(Side::Buy, dec!(100), dec!(1), &guest.client_id))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ExchangeError::Risk(RiskError::RateLimitExceeded(5))
    ));
    assert_eq!(err.status(), 429);
}

#[tokio::test]
async fn cancel_owned_only_cancels_the_owners_order() {
    let s = state();
    let order = limit(Side::Buy, dec!(100), dec!(5), "alice");
    let id = order.id;
    s.submit(order).await.unwrap();

    assert!(
        s.cancel_owned(id, SYM, Side::Buy, "bob")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(book(&s).await.bids, vec![(dec!(100), dec!(5))]);
    assert!(
        s.cancel_owned(id, SYM, Side::Buy, "alice")
            .await
            .unwrap()
            .is_some()
    );
    assert!(book(&s).await.bids.is_empty());
}

#[tokio::test]
async fn rejected_owned_cancel_is_not_logged() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();
    let live = state();
    *live.wal.lock().unwrap() = Some(WriteAheadLog::open(path).unwrap());
    let order = limit(Side::Buy, dec!(100), dec!(5), "alice");
    let id = order.id;
    live.submit(order).await.unwrap();
    live.cancel_owned(id, SYM, Side::Buy, "bob").await.unwrap();

    let entries = WriteAheadLog::replay(path).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0], WalEntry::OrderSubmitted { .. }));
}

#[tokio::test]
async fn replay_restores_registered_guest_and_resting_order() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();

    let live = state();
    *live.wal.lock().unwrap() = Some(WriteAheadLog::open(path).unwrap());
    let guest = register_guest(&live, "Replay Me");
    let resting = limit(Side::Sell, dec!(105), dec!(8), &guest.client_id);
    let resting_id = resting.id;
    live.submit(resting).await.unwrap();

    let (_, entries) = WriteAheadLog::open_for_recovery(path).unwrap();
    assert!(matches!(entries[0], WalEntry::ParticipantRegistered { .. }));
    let recovered = state();
    let stats = recovered.replay(entries).await.unwrap();
    assert_eq!((stats.applied, stats.skipped), (2, 0));

    assert_eq!(recovered.participant(&guest.client_id), Some(guest.clone()));
    {
        let risk = recovered.risk.lock().unwrap();
        let limits = &risk.participant_limits[&guest.client_id];
        assert_eq!(limits.max_notional_per_order, dec!(100_000));
        assert_eq!(limits.max_position, dec!(10_000));
        assert_eq!(limits.max_orders_per_second, 5);
    }
    assert_eq!(book(&recovered).await.asks, vec![(dec!(105), dec!(8))]);
    assert!(
        recovered
            .cancel_owned(resting_id, SYM, Side::Sell, &guest.client_id)
            .await
            .unwrap()
            .is_some()
    );
}
