use crate::handlers::{book, health, orders, session};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Thande Stonks Exchange",
        version = "0.1.0",
        description = "Price-time priority continuous matching engine. \
            Pre-registered clients: `demo`, `trader1`, `trader2`.",
        contact(name = "Roy Thande"),
    ),
    paths(
        health::healthz,
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
        (name = "book", description = "Order book snapshots"),
    )
)]
pub struct ApiDoc;
