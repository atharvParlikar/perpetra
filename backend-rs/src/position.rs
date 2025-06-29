use rust_decimal::Decimal;
use std::{collections::HashMap, time::Instant};

use crate::order::Side;

pub struct Position {
    pub user_id: String,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub margin: Decimal,
    pub realized_pnl: Decimal,
}

pub type PositionMap = HashMap<String, Position>;

#[derive(Debug)]
pub struct Trade {
    pub long_id: String,
    pub short_id: String,
    pub amount: Decimal,
    pub price: Decimal,
}

pub enum EngineEvent {
    Trade(Trade),
    PositionClosed { user_id: String },
    Liquidation { user_id: String },
}
