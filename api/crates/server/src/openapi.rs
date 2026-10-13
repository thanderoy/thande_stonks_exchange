use crate::handlers::{book, guest, health, orders, session};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Thande Stonks Exchange",
        version = "0.1.0",
        description = "Price-time priority continuous matching engine. \
            This is the admin listener: orders name their `client_id` in the body \
            (pre-registered clients: `demo`, `trader1`, `trader2`, plus any joined guest). \
            The public listener serves `/v1/join`, `/v1/me`, orders as the session's guest, \
            book, session state (read only) and the stream; it has no session control.",
        contact(name = "Roy Thande"),
    ),
    paths(
        health::healthz,
        guest::join,
        guest::me,
        orders::submit_order,
        orders::cancel_order,
        session::get_session,
        session::open_session,
        session::halt_session,
        session::resume_session,
        book::get_book,
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
            // engine types referenced in request/response bodies
            engine::order::Side,
            engine::order::OrderType,
            engine::order::TimeInForce,
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
    )
)]
pub struct ApiDoc;
