pub async fn persist_trade(pool: &PgPool, trade: &Trade) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO trades VALUES ($1, $2, $3, $4, $5, $6, $7)",
        trade.id,
        trade.symbol.as_str(),
        trade.price,
        trade.quantity,
        trade.buyer_order_id,
        trade.seller_order_id,
        trade.executed_at
    )
    .execute(pool)
    .await?;

    Ok(())

}
