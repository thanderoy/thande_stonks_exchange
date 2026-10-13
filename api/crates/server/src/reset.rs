//! Market reset: every book emptied, positions, cash, stats and trade history cleared,
//! every symbol back in pre-market, and a fresh WAL. Guests who joined recently keep
//! their accounts: the new WAL starts by registering them again.
//!
//! Runs nightly at `RESET_AT` (UTC) and on `POST /v1/admin/reset`. It holds every
//! symbol's session lock throughout, so no order lands half before and half after.

use crate::exchange::{ExchangeError, request};
use crate::market::MarketStats;
use crate::state::{AppState, ParticipantKind};
use chrono::{DateTime, NaiveTime, Utc};
use engine::session::SessionState;
use engine::shard::EngineCommand;
use market_data::MarketEvent;
use persistence::wal::WalEntry;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct ResetSummary {
    /// Guests who joined after the cutoff and carry over
    pub guests_kept: usize,
    /// Guests who joined before the cutoff; their sessions stop working
    pub guests_removed: usize,
}

impl AppState {
    /// Resets the market. Guests registered at or after `keep_guests_since` are kept;
    /// guests with no recorded join time count as joining now, so they get one more day.
    pub async fn reset_market(
        &self,
        keep_guests_since: DateTime<Utc>,
    ) -> Result<ResetSummary, ExchangeError> {
        // Fixed order, so two resets can't deadlock each other.
        let mut symbols = self.symbols.clone();
        symbols.sort();
        let mut sessions = Vec::with_capacity(symbols.len());
        for symbol in &symbols {
            sessions.push(self.session(symbol)?.lock_owned().await);
        }
        for symbol in &symbols {
            request(&self.shard(symbol)?, EngineCommand::Clear).await?;
        }

        let now = Utc::now();
        let summary = {
            // WAL before risk, as in `register_participant`.
            let mut wal = self.wal.lock().unwrap();
            let mut risk = self.risk.lock().unwrap();
            let mut kept = Vec::new();
            let mut removed = Vec::new();
            for entry in self.participants.iter() {
                if entry.kind != ParticipantKind::Guest {
                    continue;
                }
                let joined = entry.registered_at.unwrap_or(now);
                if joined >= keep_guests_since {
                    kept.push((entry.key().clone(), entry.display_name.clone(), joined));
                } else {
                    removed.push(entry.key().clone());
                }
            }
            let entries: Vec<WalEntry> = kept
                .iter()
                .filter_map(|(client_id, display_name, joined)| {
                    let limits = risk.participant_limits.get(client_id)?;
                    Some(WalEntry::ParticipantRegistered {
                        client_id: client_id.clone(),
                        display_name: display_name.clone(),
                        max_notional_per_order: limits.max_notional_per_order,
                        max_position: limits.max_position,
                        max_orders_per_second: limits.max_orders_per_second,
                        registered_at: Some(*joined),
                    })
                })
                .collect();
            if let Some(wal) = wal.as_mut() {
                wal.rotate(&entries).map_err(ExchangeError::Wal)?;
            }
            for client_id in &removed {
                risk.participant_limits.remove(client_id);
                self.participants.remove(client_id);
            }
            for limits in risk.participant_limits.values_mut() {
                limits.current_position.clear();
            }
            for (client_id, _, joined) in &kept {
                if let Some(mut info) = self.participants.get_mut(client_id) {
                    info.registered_at = Some(*joined);
                }
            }
            ResetSummary {
                guests_kept: kept.len(),
                guests_removed: removed.len(),
            }
        };
        *self.market.lock().unwrap() = MarketStats::default();

        self.reset_epoch.fetch_add(1, Ordering::SeqCst);
        self.bus.publish(MarketEvent::MarketReset);
        for (symbol, session) in symbols.iter().zip(sessions.iter_mut()) {
            session.state = SessionState::PreMarket;
            self.publish_session(symbol, &session.state);
            self.mark_book_dirty(symbol);
        }
        Ok(summary)
    }

    /// Resets the market every day at `at` (UTC), keeping guests who joined within
    /// `keep_guests_for`.
    pub fn spawn_daily_reset(self: &Arc<Self>, at: NaiveTime, keep_guests_for: chrono::Duration) {
        let state = self.clone();
        tokio::spawn(async move {
            loop {
                let now = Utc::now();
                let next = next_occurrence(now, at);
                tracing::info!("next market reset at {next}");
                let wait = (next - now).to_std().unwrap_or_default();
                tokio::time::sleep(wait).await;
                match state.reset_market(Utc::now() - keep_guests_for).await {
                    Ok(s) => tracing::info!(
                        "market reset: {} guests kept, {} removed",
                        s.guests_kept,
                        s.guests_removed
                    ),
                    Err(e) => tracing::error!("market reset failed: {e}"),
                }
            }
        });
    }
}

/// The first time after `now` (strictly) that the UTC clock reads `at`.
pub fn next_occurrence(now: DateTime<Utc>, at: NaiveTime) -> DateTime<Utc> {
    let today = now.date_naive().and_time(at).and_utc();
    if today > now {
        today
    } else {
        today + chrono::Duration::days(1)
    }
}

/// Parses `RESET_AT`: `HH:MM` in UTC.
pub fn parse_reset_at(raw: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(raw.trim(), "%H:%M")
        .map_err(|_| format!("RESET_AT must be HH:MM in UTC, got {raw:?}"))
}
