use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::{collections::HashMap, fmt, time::Instant};

use crate::order::Side;

pub struct Position {
    pub user_id: String,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub margin: Decimal,
    pub realized_pnl: Decimal,
}

pub type PositionMap = HashMap<String, Position>;

pub struct PositionTracker {
    positions: PositionMap,
}

#[derive(Debug)]
pub struct Trade {
    pub long_id: String,
    pub short_id: String,
    pub amount: Decimal,
    pub price: Decimal,
}

impl fmt::Display for Trade {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "[TRADE] {} bought {} @ {} from {}",
            self.long_id, self.amount, self.price, self.short_id
        )
    }
}

pub enum EngineEvent {
    Trade(Trade),
    PositionClosed { user_id: String },
    Liquidation { user_id: String },
}

impl PositionTracker {
    pub fn new() -> PositionTracker {
        PositionTracker {
            positions: PositionMap::new(),
        }
    }

    pub fn update_position(&mut self, trade: Trade) {
        use std::collections::hash_map::Entry;

        match self.positions.entry(trade.long_id.clone()) {
            Entry::Occupied(mut entry) => {
                let position = entry.get_mut();
                let new_entry_price = (position.entry_price * position.size
                    + trade.amount * trade.price)
                    / (trade.amount + position.size);
                position.entry_price = new_entry_price;
                position.size += trade.amount;
                position.margin += trade.price * trade.amount;
            }
            Entry::Vacant(entry) => {
                entry.insert(Position {
                    user_id: trade.long_id.clone(),
                    entry_price: trade.price,
                    size: trade.amount,
                    margin: trade.price * trade.amount,
                    realized_pnl: dec!(0),
                });
            }
        }

        match self.positions.entry(trade.short_id.clone()) {
            Entry::Occupied(mut entry) => {
                let position = entry.get_mut();
                let new_entry_price = (position.entry_price * position.size
                    + trade.amount * trade.price)
                    / (trade.amount + position.size);
                position.entry_price = new_entry_price;
                position.size -= trade.amount;
                position.margin += trade.price * trade.amount;
            }
            Entry::Vacant(entry) => {
                entry.insert(Position {
                    user_id: trade.long_id.clone(),
                    entry_price: trade.price,
                    size: -trade.amount,
                    margin: trade.price * trade.amount,
                    realized_pnl: dec!(0),
                });
            }
        }
    }
}
