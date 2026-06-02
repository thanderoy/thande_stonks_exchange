use engine::matcher::Trade;
use sqlx::PgPool;

pub async fn connect(database_url: &str) -> sqlx::Result<PgPool> {
    PgPool::connect(database_url).await
}

pub async fn persist_trade(pool: &PgPool, trade: &Trade) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO trades (id, symbol, price, quantity, buyer_order_id, seller_order_id, executed_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(trade.id.0)
    .bind(trade.symbol.as_str())
    .bind(trade.price)
    .bind(trade.quantity)
    .bind(trade.buyer_order_id.0)
    .bind(trade.seller_order_id.0)
    .bind(trade.executed_at)
    .execute(pool)
    .await?;
    Ok(())
}
