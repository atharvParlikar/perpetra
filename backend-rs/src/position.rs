use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::{
    collections::{hash_map::Entry, HashMap},
    fmt,
    sync::Arc,
};
use tokio::sync::{mpsc::Sender, oneshot};
use uuid::Uuid;

use crate::{
    oracle::BtcPrice,
    order::{
        Order,
        OrderType::MARKET,
        Side::{ASK, BID},
    },
    Message,
};

pub struct Position {
    pub user_id: String,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub margin: Decimal,
    pub unrealized_pnl: Decimal,
}

pub type PositionMap = HashMap<String, Position>;

pub type BookLiquidationTx = Arc<Sender<Message>>;

pub struct PositionTracker {
    positions: PositionMap,
    book_liquidation_tx: BookLiquidationTx,
}

const LIQUIDATION_THRESHOLD: Decimal = dec!(0.8);

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
    Oracle { mark_price: Decimal },
}

impl PositionTracker {
    pub fn new(book_liquidation_tx: BookLiquidationTx) -> PositionTracker {
        PositionTracker {
            positions: PositionMap::new(),
            book_liquidation_tx,
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
                if (position.size < dec!(0)) {
                    position.margin -= trade.price * trade.amount;
                } else {
                    position.margin += trade.price * trade.amount;
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(Position {
                    user_id: trade.long_id.clone(),
                    entry_price: trade.price,
                    size: trade.amount,
                    margin: trade.price * trade.amount,
                    unrealized_pnl: dec!(0),
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
                if (position.size > dec!(0)) {
                    position.margin += trade.price * trade.amount;
                } else {
                    position.margin -= trade.price * trade.amount;
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(Position {
                    user_id: trade.long_id.clone(),
                    entry_price: trade.price,
                    size: -trade.amount,
                    margin: trade.price * trade.amount,
                    unrealized_pnl: dec!(0),
                });
            }
        }
    }

    fn liquidate(&mut self, user_id: &String) {
        let (resp_tx, resp_rx) = oneshot::channel();

        if let Some(position) = self.positions.get(user_id) {
            let mut order = Order {
                id: Uuid::new_v4().to_string(),
                user_id: position.user_id.clone(),
                amount: position.size,
                price: dec!(0),
                order_type: MARKET,
                side: ASK,

                responder: Some(resp_tx),
            };

            if position.size < dec!(0) {
                order.side = BID;
            }

            self.book_liquidation_tx.send(Message::Order(order));
        }
    }

    // Update P&L and process liquidation if any
    pub fn update_risk(&mut self, mark_price: Decimal) {
        let mut positions_to_liquidate: Vec<String> = Vec::new();
        for (_, position) in self.positions.iter_mut() {
            position.unrealized_pnl = position.size * (mark_price - position.entry_price);

            if (position.margin + position.unrealized_pnl)
                <= position.margin * LIQUIDATION_THRESHOLD
            {
                println!("[LIQUIDATION] {}", position.user_id);
                positions_to_liquidate.push(position.user_id.clone());
            }
        }

        for user_id in &positions_to_liquidate {
            self.liquidate(user_id);
        }
    }
}
