use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::{
    mpsc::{Sender, UnboundedReceiver},
    Mutex,
};
use uuid::Uuid;

use crate::{
    broadcast_trade,
    domain::order::{
        Order,
        OrderType::MARKET,
        Side::{ASK, BID},
    },
    types::OrderBookMessage,
    SocketList,
};

use tokio::sync::mpsc;

pub struct Position {
    pub user_id: String,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub margin: Decimal,
    pub unrealized_pnl: Decimal,
}

pub type PositionMap = HashMap<String, Position>;

pub type BookLiquidationTx = Sender<OrderBookMessage>;

pub struct PositionTracker {
    positions: PositionMap,
    book_liquidation_tx: BookLiquidationTx,
}

const LIQUIDATION_THRESHOLD: Decimal = dec!(0.8);

#[derive(Debug, Clone, Serialize)]
pub struct Trade {
    pub long_id: String,
    pub short_id: String,
    pub long_leverage: Decimal,
    pub short_leverage: Decimal,
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
}

fn adjust_for_leverage(margin: Decimal, leverage: Decimal) -> Decimal {
    margin / leverage
}

impl PositionTracker {
    pub fn new(book_liquidation_tx: BookLiquidationTx) -> PositionTracker {
        PositionTracker {
            positions: PositionMap::new(),
            book_liquidation_tx,
        }
    }

    pub fn update_position(&mut self, trade: &Trade) {
        use std::collections::hash_map::Entry;

        match self.positions.entry(trade.long_id.clone()) {
            Entry::Occupied(mut entry) => {
                let position = entry.get_mut();
                let new_entry_price = (position.entry_price * position.size
                    + trade.amount * trade.price)
                    / (trade.amount + position.size);
                position.entry_price = new_entry_price;
                position.size += trade.amount;
                // new trade going in the same direction, i.e trade_1: long, trade_2: long
                if (position.size < dec!(0)) {
                    position.margin +=
                        adjust_for_leverage(trade.price * trade.amount, trade.long_leverage)
                } else {
                    position.margin -=
                        adjust_for_leverage(trade.price * trade.amount, trade.long_leverage);
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(Position {
                    user_id: trade.long_id.clone(),
                    entry_price: trade.price,
                    size: trade.amount,
                    margin: adjust_for_leverage(trade.price * trade.amount, trade.long_leverage),
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

                // new trade going in the same direction, i.e trade_1: short, trade_2: short
                if (position.size > dec!(0)) {
                    position.margin +=
                        adjust_for_leverage(trade.price * trade.amount, trade.short_leverage)
                } else {
                    position.margin -=
                        adjust_for_leverage(trade.price * trade.amount, trade.short_leverage);
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(Position {
                    user_id: trade.short_id.clone(),
                    entry_price: trade.price,
                    size: -trade.amount,
                    margin: adjust_for_leverage(trade.price * trade.amount, trade.short_leverage),
                    unrealized_pnl: dec!(0),
                });
            }
        }
    }

    fn liquidate(&mut self, user_id: &String) {
        if let Some(position) = self.positions.get(user_id) {
            let mut order = Order {
                id: Uuid::new_v4().to_string(),
                user_id: position.user_id.clone(),
                amount: position.size,
                price: dec!(0),
                order_type: MARKET,
                side: ASK,
                leverage: dec!(1),

                responder: None,
            };

            if position.size < dec!(0) {
                order.side = BID;
            }

            self.book_liquidation_tx
                .send(OrderBookMessage::Order(order));
        }
    }

    // Update P&L and process liquidation if any
    pub fn update_risk(&mut self, mark_price: Decimal) {
        let mut positions_to_liquidate: Vec<String> = Vec::new();
        for (user_id, position) in self.positions.iter_mut() {
            position.unrealized_pnl = position.size * (mark_price - position.entry_price);
            if (position.margin + position.unrealized_pnl)
                <= position.margin * LIQUIDATION_THRESHOLD
            {
                println!("dis nigga {} bout to be liquidated", position.user_id);
                positions_to_liquidate.push(position.user_id.clone());
            }
        }

        for user_id in &positions_to_liquidate {
            self.liquidate(user_id);
            self.positions.remove_entry(user_id);
            println!("=====\n{:?}\n", self.positions.keys());
        }
    }
}

pub async fn run_position_loop(
    mut oracle_rx: UnboundedReceiver<Decimal>,
    mut position_rx: mpsc::UnboundedReceiver<EngineEvent>,
    mut positions: PositionTracker,
    sockets: Arc<Mutex<SocketList>>,
) {
    loop {
        tokio::select! {
            maybe_mark_price = oracle_rx.recv() => {
                match maybe_mark_price {
                    Some(mark_price) => {
                        positions.update_risk(mark_price);
                    }
                    None => {
                        break;
                    }
                }
            }
            maybe_event = position_rx.recv() => {
                match maybe_event {
                    Some(EngineEvent::Trade(trade)) => {
                        positions.update_position(&trade);
                        broadcast_trade(trade.clone(), sockets.clone()).await;
                    }
                    None => {
                        break;
                    }
                }
            }
        }
    }
}
