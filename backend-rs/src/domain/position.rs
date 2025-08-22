use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::{
    mpsc::{Sender, UnboundedReceiver},
    Mutex,
};
use uuid::Uuid;

use std::collections::hash_map::Entry;

use crate::{
    broadcast_trade,
    domain::{
        oracle::BtcPrice,
        order::{
            Order,
            OrderType::MARKET,
            Side::{ASK, BID},
        },
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
    mark_price: Decimal,
    last_traded_price: Decimal,
    current_funding_rate: Decimal,
    funding_rate_window: Vec<Decimal>,
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
            last_traded_price: dec!(0),
            mark_price: dec!(0),
            current_funding_rate: dec!(0),
            funding_rate_window: Vec::new(),
        }
    }

    pub fn update_position(&mut self, trade: &Trade) {
        let mut positions_to_remove: Vec<String> = Vec::new();

        match self.positions.entry(trade.long_id.clone()) {
            Entry::Occupied(mut entry) => {
                let position = entry.get_mut();
                let new_size = trade.amount + position.size;
                if new_size.is_zero() {
                    positions_to_remove.push(trade.long_id.clone());
                } else {
                    let new_entry_price = (position.entry_price * position.size
                        + trade.amount * trade.price)
                        / new_size;
                    position.entry_price = new_entry_price;
                    position.size += trade.amount;
                }
                // new trade going in the same direction, i.e trade_1: long, trade_2: long
                if position.size < dec!(0) {
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

                let new_size = trade.amount + position.size;
                if new_size.is_zero() {
                    positions_to_remove.push(trade.long_id.clone());
                } else {
                    let new_entry_price = (position.entry_price * position.size
                        + trade.amount * trade.price)
                        / new_size;
                    position.entry_price = new_entry_price;
                    position.size += trade.amount;
                }

                // new trade going in the same direction, i.e trade_1: short, trade_2: short
                if position.size > dec!(0) {
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

        for position_id in positions_to_remove {
            self.positions.remove_entry(&position_id);
        }
    }

    async fn liquidate(&mut self, user_id: &String) {
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

            if let Err(error) = self
                .book_liquidation_tx
                .send(OrderBookMessage::Order(order))
                .await
            {
                eprintln!("{}", error);
            }
        }
    }

    pub fn update_funding_rate(&mut self, index_price: Decimal) {
        let dampning_factor = dec!(0.05);
        let funding_rate_window_duration = 60; // minutes

        let premium = (self.last_traded_price - index_price) / index_price;
        let raw_funding_rate = premium * dampning_factor;
        let min_clamp = dec!(-0.00075);
        let max_clamp = dec!(0.00075);
        let current_funding_rate = (raw_funding_rate.min(min_clamp)).max(max_clamp);

        self.current_funding_rate = current_funding_rate;

        if self.funding_rate_window.len() >= funding_rate_window_duration {
            self.funding_rate_window
                .drain(1..self.funding_rate_window.len());
        }

        self.funding_rate_window.push(current_funding_rate);
    }

    pub fn update_mark_price(&mut self, index_price: Decimal) {
        self.mark_price = index_price * (dec!(1) + self.current_funding_rate);
    }

    // Update P&L and process liquidation if any
    pub async fn update_risk(&mut self) {
        let mut positions_to_liquidate: Vec<String> = Vec::new();
        for (_, position) in self.positions.iter_mut() {
            position.unrealized_pnl = position.size * (self.mark_price - position.entry_price);
            if (position.margin + position.unrealized_pnl)
                <= position.margin * LIQUIDATION_THRESHOLD
            {
                positions_to_liquidate.push(position.user_id.clone());
            }
        }

        for user_id in &positions_to_liquidate {
            self.liquidate(user_id).await;
        }
    }
}

pub async fn run_position_loop(
    mut oracle_rx: UnboundedReceiver<BtcPrice>,
    mut position_rx: mpsc::UnboundedReceiver<EngineEvent>,
    mut positions: PositionTracker,
    sockets: Arc<Mutex<SocketList>>,
) {
    loop {
        tokio::select! {
            maybe_oracle_event = oracle_rx.recv() => {
                match maybe_oracle_event {
                    Some(oracle_event) => {
                        positions.update_funding_rate(oracle_event.price_usd);
                        // in production instead of moving average use just the price, not the moving average
                        positions.update_mark_price(oracle_event.moving_average);
                        positions.update_risk().await;
                    }
                    None => {
                        break;
                    }
                }
            }
            maybe_event = position_rx.recv() => {
                match maybe_event {
                    Some(EngineEvent::Trade(trade)) => {
                        positions.last_traded_price = trade.price;
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
