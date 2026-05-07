CREATE TABLE trades (
    id UUID PRIMARY KEY,
    symbol TEXT NOT NULL,
    price NUMERIC(18,8) NOT NULL,
    quantity NUMERIC(18,8) NOT NULL,
    buyer_order_id UUID NOT NULL,
    seller_order_id UUID NOT NULL,
    executed_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX treade_symmbol_time_idx ON trades(symbol, executed_at DESC);
