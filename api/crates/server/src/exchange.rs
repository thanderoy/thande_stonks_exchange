//! Every state change goes through here, live or replayed from the WAL.
//!
//! Live operations check, append to the WAL, then apply; replay applies only. Both run
//! under the symbol's session lock, so a symbol's WAL entries are in the order its shard
//! applied them and replay reproduces the same books, positions and sessions.

use crate::state::AppState;
use actix_web::http::StatusCode;
use chrono::Utc;
use common::{OrderId, Symbol};
use engine::matcher::Trade;
use engine::order::{Order, OrderType, Side, TimeInForce};
use engine::session::{SessionState, TradingSession};
use engine::shard::EngineCommand;
use market_data::MarketEvent;
use persistence::wal::WalEntry;
use risk::RiskError;
use rust_decimal::Decimal;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, mpsc};

#[derive(Debug)]
pub enum ExchangeError {
    UnknownSymbol(String),
    NotAcceptingOrders(String, SessionState),
    PreOpenRequiresRestingLimit,
    Risk(RiskError),
    Wal(std::io::Error),
    ShardUnavailable,
}

impl ExchangeError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::UnknownSymbol(_) => StatusCode::NOT_FOUND,
            Self::NotAcceptingOrders(..) => StatusCode::CONFLICT,
            Self::Risk(RiskError::RateLimitExceeded(_)) => StatusCode::TOO_MANY_REQUESTS,
            Self::PreOpenRequiresRestingLimit | Self::Risk(_) => StatusCode::BAD_REQUEST,
            Self::Wal(_) | Self::ShardUnavailable => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSymbol(s) => write!(f, "symbol {s} not found"),
            Self::NotAcceptingOrders(s, state) => {
                write!(
                    f,
                    "{s} is not accepting orders (session {})",
                    state_name(state)
                )
            }
            Self::PreOpenRequiresRestingLimit => {
                write!(
                    f,
                    "before the open only GTC or Day limit orders are accepted"
                )
            }
            Self::Risk(e) => write!(f, "{e}"),
            Self::Wal(e) => write!(f, "write-ahead log failed: {e}"),
            Self::ShardUnavailable => write!(f, "shard unavailable"),
        }
    }
}

/// Short state name, as published in `session_state` market events.
pub fn state_name(state: &SessionState) -> &'static str {
    match state {
        SessionState::PreMarket => "PreMarket",
        SessionState::Open => "Open",
        SessionState::Halted { .. } => "Halted",
        SessionState::PostMarket => "PostMarket",
        SessionState::Closed => "Closed",
    }
}

/// A participant created at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub client_id: String,
    pub display_name: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct ReplayStats {
    pub applied: usize,
    /// Entries for symbols not configured in this run.
    pub skipped: usize,
}

/// Pre-open orders can only rest, so they must be limit orders that are allowed to rest.
fn can_rest_pre_open(order: &Order) -> bool {
    !matches!(order.order_type, OrderType::Market)
        && matches!(
            order.time_in_force,
            TimeInForce::GoodTillCanceled | TimeInForce::Day
        )
}

async fn request<T>(
    shard: &mpsc::Sender<EngineCommand>,
    command: impl FnOnce(mpsc::Sender<T>) -> EngineCommand,
) -> Result<T, ExchangeError> {
    let (tx, mut rx) = mpsc::channel(1);
    shard
        .send(command(tx))
        .await
        .map_err(|_| ExchangeError::ShardUnavailable)?;
    rx.recv().await.ok_or(ExchangeError::ShardUnavailable)
}

impl AppState {
    fn session(&self, symbol: &str) -> Result<Arc<Mutex<TradingSession>>, ExchangeError> {
        self.sessions
            .get(symbol)
            .map(|s| s.clone())
            .ok_or_else(|| ExchangeError::UnknownSymbol(symbol.to_string()))
    }

    fn shard(&self, symbol: &str) -> Result<mpsc::Sender<EngineCommand>, ExchangeError> {
        self.shards
            .get(symbol)
            .map(|s| s.clone())
            .ok_or_else(|| ExchangeError::UnknownSymbol(symbol.to_string()))
    }

    fn log(&self, entry: &WalEntry) -> Result<(), ExchangeError> {
        match self.wal.lock().unwrap().as_mut() {
            Some(wal) => wal.append(entry).map_err(ExchangeError::Wal),
            None => Ok(()),
        }
    }

    /// Session gate → risk check → WAL → shard. Returns any immediate fills.
    pub async fn submit(&self, order: Order) -> Result<Vec<Trade>, ExchangeError> {
        let symbol = order.symbol.as_str().to_string();
        let session = self.session(&symbol)?;
        let session = session.lock().await;
        if !session.can_accept_orders() {
            return Err(ExchangeError::NotAcceptingOrders(
                symbol,
                session.state.clone(),
            ));
        }
        if !session.can_match() && !can_rest_pre_open(&order) {
            return Err(ExchangeError::PreOpenRequiresRestingLimit);
        }
        self.risk
            .lock()
            .unwrap()
            .check_live(&order, Instant::now())
            .map_err(ExchangeError::Risk)?;
        self.log(&WalEntry::OrderSubmitted {
            order: order.clone(),
        })?;
        self.apply_submit(order, session.can_match()).await
    }

    /// Cancels are accepted in every session state. Returns the removed order, if it was resting.
    pub async fn cancel(
        &self,
        order_id: OrderId,
        symbol: &str,
        side: Side,
    ) -> Result<Option<Order>, ExchangeError> {
        let session = self.session(symbol)?;
        let _session = session.lock().await;
        self.log(&WalEntry::OrderCancelled {
            order_id: order_id.0,
            symbol: symbol.to_string(),
            side: side.clone(),
            timestamp: Utc::now().to_rfc3339(),
        })?;
        self.apply_cancel(order_id, symbol, side).await
    }

    /// Like [`cancel`](Self::cancel), but only if the resting order belongs to `owner`.
    /// Someone else's order looks exactly like a missing one: `Ok(None)`, nothing logged.
    /// The ownership check and the cancel run under one session lock, so the order can't
    /// change hands in between, and only authorised cancels reach the WAL.
    pub async fn cancel_owned(
        &self,
        order_id: OrderId,
        symbol: &str,
        side: Side,
        owner: &str,
    ) -> Result<Option<Order>, ExchangeError> {
        let session = self.session(symbol)?;
        let _session = session.lock().await;
        let shard = self.shard(symbol)?;
        let resting_owner = request(&shard, |tx| {
            EngineCommand::OrderOwner(order_id, side.clone(), tx)
        })
        .await?;
        if resting_owner.as_deref() != Some(owner) {
            return Ok(None);
        }
        self.log(&WalEntry::OrderCancelled {
            order_id: order_id.0,
            symbol: symbol.to_string(),
            side: side.clone(),
            timestamp: Utc::now().to_rfc3339(),
        })?;
        self.apply_cancel(order_id, symbol, side).await
    }

    /// Creates a participant with a fresh `g-…` id and the given limits: WAL, then apply.
    pub fn register_participant(
        &self,
        display_name: String,
        max_notional_per_order: Decimal,
        max_position: Decimal,
        max_orders_per_second: u32,
    ) -> Result<Participant, ExchangeError> {
        let client_id = loop {
            let id = crate::guest::new_client_id();
            if !self
                .risk
                .lock()
                .unwrap()
                .participant_limits
                .contains_key(&id)
            {
                break id;
            }
        };
        self.log(&WalEntry::ParticipantRegistered {
            client_id: client_id.clone(),
            display_name: display_name.clone(),
            max_notional_per_order,
            max_position,
            max_orders_per_second,
        })?;
        Ok(self.apply_register(
            client_id,
            display_name,
            max_notional_per_order,
            max_position,
            max_orders_per_second,
        ))
    }

    /// Display name of a runtime-registered participant.
    pub fn participant(&self, client_id: &str) -> Option<Participant> {
        self.display_names.get(client_id).map(|name| Participant {
            client_id: client_id.to_string(),
            display_name: name.clone(),
        })
    }

    /// Applies a session transition. Entering Open runs the opening auction, whose trades are returned.
    pub async fn transition(
        &self,
        symbol: &str,
        change: impl FnOnce(&mut TradingSession),
    ) -> Result<(SessionState, Vec<Trade>), ExchangeError> {
        let session = self.session(symbol)?;
        let mut session = session.lock().await;
        let mut next = TradingSession {
            state: session.state.clone(),
        };
        change(&mut next);
        self.log(&WalEntry::SessionStateChanged {
            symbol: symbol.to_string(),
            state: next.state.clone(),
        })?;
        let trades = self
            .apply_session(symbol, &mut session, next.state.clone())
            .await?;
        Ok((next.state, trades))
    }

    /// Rebuilds state from WAL entries. Call before the WAL is attached and before serving.
    pub async fn replay(&self, entries: Vec<WalEntry>) -> Result<ReplayStats, ExchangeError> {
        let mut stats = ReplayStats::default();
        for entry in entries {
            if let WalEntry::ParticipantRegistered {
                client_id,
                display_name,
                max_notional_per_order,
                max_position,
                max_orders_per_second,
            } = entry
            {
                self.apply_register(
                    client_id,
                    display_name,
                    max_notional_per_order,
                    max_position,
                    max_orders_per_second,
                );
                stats.applied += 1;
                continue;
            }
            let symbol = match &entry {
                WalEntry::OrderSubmitted { order } => order.symbol.as_str().to_string(),
                WalEntry::OrderCancelled { symbol, .. }
                | WalEntry::SessionStateChanged { symbol, .. } => symbol.clone(),
                WalEntry::TradeExecuted { .. } => continue,
                WalEntry::ParticipantRegistered { .. } => unreachable!(),
            };
            let Ok(session) = self.session(&symbol) else {
                stats.skipped += 1;
                continue;
            };
            let mut session = session.lock().await;
            match entry {
                WalEntry::OrderSubmitted { order } => {
                    self.apply_submit(order, session.can_match()).await?;
                }
                WalEntry::OrderCancelled {
                    order_id,
                    symbol,
                    side,
                    ..
                } => {
                    self.apply_cancel(OrderId(order_id), &symbol, side).await?;
                }
                WalEntry::SessionStateChanged { symbol, state } => {
                    self.apply_session(&symbol, &mut session, state).await?;
                }
                WalEntry::TradeExecuted { .. } | WalEntry::ParticipantRegistered { .. } => {
                    unreachable!()
                }
            }
            stats.applied += 1;
        }
        Ok(stats)
    }

    /// Registers the participant with the risk engine and records its display name.
    fn apply_register(
        &self,
        client_id: String,
        display_name: String,
        max_notional_per_order: Decimal,
        max_position: Decimal,
        max_orders_per_second: u32,
    ) -> Participant {
        self.risk.lock().unwrap().register_participant(
            client_id.clone(),
            max_notional_per_order,
            max_position,
            max_orders_per_second,
        );
        self.display_names
            .insert(client_id.clone(), display_name.clone());
        Participant {
            client_id,
            display_name,
        }
    }

    async fn apply_submit(
        &self,
        order: Order,
        can_match: bool,
    ) -> Result<Vec<Trade>, ExchangeError> {
        let shard = self.shard(order.symbol.as_str())?;
        if !can_match {
            request(&shard, |tx| EngineCommand::Rest(order, tx)).await?;
            return Ok(Vec::new());
        }
        let trades = request(&shard, |tx| EngineCommand::Submit(order, tx)).await?;
        self.record_positions(&trades);
        Ok(trades)
    }

    async fn apply_cancel(
        &self,
        order_id: OrderId,
        symbol: &str,
        side: Side,
    ) -> Result<Option<Order>, ExchangeError> {
        let shard = self.shard(symbol)?;
        request(&shard, |tx| EngineCommand::Cancel(order_id, side, tx)).await
    }

    async fn apply_session(
        &self,
        symbol: &str,
        session: &mut TradingSession,
        state: SessionState,
    ) -> Result<Vec<Trade>, ExchangeError> {
        session.state = state;
        if !session.can_match() {
            return Ok(Vec::new());
        }
        // Uncrossing an uncrossed book is a no-op, so this is safe on every entry to Open.
        let shard = self.shard(symbol)?;
        let trades = request(&shard, EngineCommand::Uncross).await?;
        self.record_positions(&trades);
        Ok(trades)
    }

    fn record_positions(&self, trades: &[Trade]) {
        let mut risk = self.risk.lock().unwrap();
        for trade in trades {
            risk.record_trade(trade);
        }
    }

    /// Side effects of live trades: audit WAL record, market data event, Postgres ledger.
    pub async fn emit_trades(&self, trades: &[Trade]) {
        for trade in trades {
            let audit = WalEntry::TradeExecuted {
                trade_id: trade.id.0,
                symbol: trade.symbol.to_string(),
                price: trade.price.to_string(),
                quantity: trade.quantity.to_string(),
                buyer_order_id: trade.buyer_order_id.0,
                seller_order_id: trade.seller_order_id.0,
                executed_at: trade.executed_at.to_rfc3339(),
            };
            if let Err(e) = self.log(&audit) {
                tracing::warn!("trade audit record not written: {e}");
            }
            self.bus.publish(MarketEvent::Trade(trade.clone()));
            let persisted = match &self.pool {
                Some(pool) => persistence::ledger::persist_trade(pool, trade).await,
                None => Ok(()),
            };
            if let Err(e) = persisted {
                tracing::warn!("trade {} not persisted to ledger: {e}", trade.id.0);
            }
        }
    }

    pub fn publish_session(&self, symbol: &str, state: &SessionState) {
        self.bus.publish(MarketEvent::SessionState {
            symbol: Symbol::new(symbol),
            state: state_name(state).to_string(),
        });
    }
}
