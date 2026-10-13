use crate::handlers::{admin, book, guest, health, market, orders, sandbox, session};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Thande Stonks Exchange",
        version = "0.1.0",
        description = "Price-time priority continuous matching engine. \
            This is the admin listener: orders name their `client_id` in the body \
            (pre-registered clients: `demo`, `trader1`, `trader2`, plus any joined guest). \
            The public listener serves `/v1/join`, `/v1/me` (with the caller's orders and \
            account), orders as the session's guest, the market reads (book, session state, \
            symbols, trades, leaderboard) and the stream, where order events are sent only \
            to their owner. Each guest can have a private sandbox symbol (`/v1/me/sandbox`) \
            that only they see, trade and control; there is no other session control and \
            no `/v1/admin`.",
        contact(name = "Roy Thande"),
    ),
    paths(
        health::healthz,
        guest::join,
        guest::me,
        guest::my_orders,
        guest::my_account,
        orders::submit_order,
        orders::cancel_order,
        session::get_session,
        session::open_session,
        session::halt_session,
        session::resume_session,
        book::get_book,
        market::symbols,
        market::trades,
        market::leaderboard,
        admin::reset,
        sandbox::get,
        sandbox::create,
        sandbox::delete,
        sandbox::seed,
    ),
    components(
        schemas(
            // api handler types
            health::HealthResponse,
            guest::JoinRequest,
            guest::IdentityResponse,
            orders::SubmitOrderRequest,
            orders::SubmitOrderResponse,
            orders::CancelOrderResponse,
            orders::ErrorResponse,
            session::SessionStateResponse,
            session::HaltBody,
            session::ErrorResponse,
            book::BookResponse,
            guest::OpenOrder,
            guest::OpenOrdersResponse,
            market::TradesResponse,
            market::LeaderboardResponse,
            crate::market::SymbolSummary,
            crate::market::Account,
            crate::market::Position,
            crate::market::LeaderboardEntry,
            crate::state::ParticipantKind,
            crate::reset::ResetSummary,
            crate::market::SymbolKind,
            crate::sandbox::SandboxResponse,
            crate::sandbox::SeedResponse,
            // engine types referenced in request/response bodies
            engine::order::Side,
            engine::order::OrderType,
            engine::order::TimeInForce,
            engine::order::OrderStatus,
            engine::matcher::Trade,
            engine::matcher::TradeId,
            engine::session::SessionState,
            engine::session::HaltReason,
            // common types
            common::OrderId,
            common::Symbol,
        )
    ),
    tags(
        (name = "orders", description = "Submit and cancel orders"),
        (name = "session", description = "Trading session lifecycle"),
        (name = "health", description = "Service health"),
        (name = "guest", description = "Guest identity (public listener only)"),
        (name = "book", description = "Order book snapshots"),
        (name = "market", description = "Symbols, recent trades and the leaderboard"),
        (name = "admin", description = "Market operations (admin listener only)"),
        (name = "sandbox", description = "The caller's private sandbox symbol (public listener only)"),
    )
)]
pub struct ApiDoc;
