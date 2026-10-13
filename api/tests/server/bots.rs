//! The in-process bots, run for real against an `AppState` with a fast tick.

use common::Symbol;
use engine::session::SessionState;
use market_data::EventBus;
use risk::{RiskEngine, SymbolLimits};
use rust_decimal_macros::dec;
use server::bots::{self, BotConfig, MAKER_ID, TAKER_ID};
use server::state::{AppState, ParticipantKind};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const SYM: &str = "AAPL";

fn state() -> Arc<AppState> {
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
        RiskEngine::new(HashMap::new(), symbol_limits),
        Arc::new(EventBus::new(1024)),
        None,
    ))
}

async fn wait_for(what: &str, mut done: impl AsyncFnMut() -> bool) {
    for _ in 0..200 {
        if done().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn bots_open_the_market_quote_both_sides_and_trade() {
    let s = state();
    bots::register(&s);
    assert_eq!(
        s.participants.get(MAKER_ID).unwrap().kind,
        ParticipantKind::Bot
    );
    assert!(
        s.participant(MAKER_ID).is_none(),
        "nobody can hold a session as a bot"
    );

    bots::spawn(
        &s,
        BotConfig {
            interval: Duration::from_millis(10),
            pre_open: Duration::from_millis(50),
        },
    );

    wait_for("the bot to open the session", async || {
        s.symbol_summaries().await[0].state == SessionState::Open
    })
    .await;
    wait_for("trades", async || s.symbol_summaries().await[0].trades >= 5).await;
    wait_for("a two-sided book", async || {
        let book = s.book(SYM).await.unwrap();
        !book.bids.is_empty() && !book.asks.is_empty()
    })
    .await;

    let book = s.book(SYM).await.unwrap();
    assert!(book.bids[0].0 < book.asks[0].0, "book is uncrossed");
    let summary = &s.symbol_summaries().await[0];
    let last = summary.last.unwrap();
    assert!(last > summary.collar_low && last < summary.collar_high);

    // The maker never leaves more than its ladder resting.
    let maker_orders = s.open_orders_in(SYM, Some(MAKER_ID)).await;
    assert!(
        maker_orders.len() <= 8,
        "{} maker orders",
        maker_orders.len()
    );
    // Both bots appear on the leaderboard once they have traded.
    let names: Vec<String> = s
        .leaderboard(10, None)
        .into_iter()
        .map(|e| e.client_id)
        .collect();
    assert!(names.contains(&TAKER_ID.to_string()));
}

#[tokio::test]
async fn bots_leave_a_halted_symbol_alone() {
    let s = state();
    bots::register(&s);
    s.transition(SYM, |t| t.open()).await.unwrap();
    s.transition(SYM, |t| t.halt(engine::session::HaltReason::TechnicalIssue))
        .await
        .unwrap();
    bots::spawn(
        &s,
        BotConfig {
            interval: Duration::from_millis(10),
            pre_open: Duration::from_millis(0),
        },
    );
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(matches!(
        s.symbol_summaries().await[0].state,
        SessionState::Halted { .. }
    ));
    assert!(s.open_orders(None).await.is_empty());
}
