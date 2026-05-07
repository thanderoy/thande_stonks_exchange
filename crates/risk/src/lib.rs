pub struct RiskEngine {
    pub participant_limits: HashMap<ClientId, ParticipantLimits>,
    pub symbol_limits: HashMap<Symbol, SymbolLimits>
}

pub struct ParticipantLimits {
    pub max_notional_per_order: Decimal,
    pub max_orders_per_second: u32,
    pub current_position: HashMap<Symbol, Decimal>,
}

impl RiskEngine {
    pub fn check(&self,order: &Order) -> Result<(), RiskError> {
        self.check_symbol_active(order)?;
        self.check_price_collar(order)?;
        self.check_notional_limit(order)?;
        self.check_position_limits(order)?;
        Ok(())
    }
}


