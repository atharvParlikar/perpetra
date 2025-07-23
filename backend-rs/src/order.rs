use std::collections::{BTreeMap, VecDeque};
use std::default;
use std::fmt::{self, write};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

use rust_decimal_macros::dec;
use tokio::sync::oneshot;

use rust_decimal::Decimal;
use OrderType::{LIMIT, MARKET};

use crate::position::{EngineEvent, Trade};

#[derive(Debug, Clone, PartialEq)]
pub enum OrderType {
    MARKET,
    LIMIT,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Side {
    BID,
    ASK,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Side::BID => write!(f, "BID"),
            Side::ASK => write!(f, "ASK"),
        }
    }
}

pub type Amount = Decimal;
pub type Price = Decimal;

pub struct Order {
    pub id: String,
    pub user_id: String,
    pub order_type: OrderType,
    pub amount: Amount,
    pub price: Price,
    pub side: Side,
    pub leverage: Decimal,

    pub responder: Option<oneshot::Sender<OrderResponse>>,
}

pub struct OrderBook {
    pub bids: BTreeMap<Price, VecDeque<Order>>,
    pub asks: BTreeMap<Price, VecDeque<Order>>,
    pub best_bid: Option<Price>,
    pub best_ask: Option<Price>,

    position_tx: crossbeam::channel::Sender<EngineEvent>,
}

#[derive(Clone)]
pub struct OrderResponse {
    pub status: String,
    pub filled: Amount,
    pub remaining: Amount,
}

impl fmt::Display for Order {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} {} {} BTC @ {}",
            self.user_id, self.side, self.amount, self.price
        )
    }
}

impl fmt::Display for OrderBook {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.bids.is_empty() && self.asks.is_empty() {
            return write!(f, "OrderBook is empty");
        }

        let mut output = String::new();

        // Process bids
        if !self.bids.is_empty() {
            output.push_str("=== BIDS ===\n");
            for (price, orders) in &self.bids {
                output.push_str(&format!("Price level: {}\n", price));
                for order in orders {
                    output.push_str(&format!("{}\n", order));
                }
                output.push_str("\n");
            }
        }

        // Process asks
        if !self.asks.is_empty() {
            output.push_str("=== ASKS ===\n\n");
            for (price, orders) in &self.asks {
                output.push_str(&format!("Price level: {}\n", price));
                for order in orders.iter().rev() {
                    output.push_str(&format!("{}\n", order));
                }
                output.push_str("\n");
            }
        }

        write!(f, "{}", output)
    }
}

impl OrderBook {
    pub fn new(position_tx: crossbeam::channel::Sender<EngineEvent>) -> Self {
        OrderBook {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            best_bid: None,
            best_ask: None,
            position_tx,
        }
    }

    pub fn insert_order(&mut self, order: Order) {
        match order.side {
            Side::BID => self.handle_buy(order),
            Side::ASK => self.handle_sell(order),
        }

        self.update_best_prices();
    }

    pub fn handle_buy(&mut self, mut order: Order) {
        let mut filled: Amount = dec!(0);

        let mut prices_to_remove: Vec<Price> = Vec::new();

        // ascending price order
        for (&price, queue) in self.asks.iter_mut() {
            if order.order_type == LIMIT && price > order.price {
                match self.bids.get_mut(&order.price) {
                    Some(buys) => {
                        if let Some(responder) = order.responder.take() {
                            let _ = responder.send(OrderResponse {
                                status: "order partially filled, remaining added to queue!"
                                    .to_string(),
                                filled,
                                remaining: order.amount,
                            });
                        }
                        buys.push_back(order);
                        return;
                    }
                    None => {
                        let mut new_queue: VecDeque<Order> = VecDeque::new();

                        if let Some(responder_tx) = order.responder.take() {
                            responder_tx.send(OrderResponse {
                                status: "could not match, added to queue!".to_string(),
                                filled: dec!(0),
                                remaining: order.amount,
                            });
                        }

                        new_queue.push_back(order);
                        self.bids.insert(price, new_queue);
                        return;
                    }
                }
            }

            while let Some(ask) = queue.front_mut() {
                let trade_amount = order.amount.min(ask.amount);
                println!(
                    "Matched BUY {} with SELL {} @ {} for {}",
                    order.id, ask.id, price, trade_amount
                );

                order.amount -= trade_amount;
                ask.amount -= trade_amount;
                filled += trade_amount;

                //  TODO: try_send does not give enough fucks to try again if the buffer is full
                //        it will simply throw an error, catch it and either drop the trade,
                //        see why tf is is blocked as it shouldn't as it has 10k limit or try
                //        again.

                // let the position tracker know the trade just happened here
                self.position_tx.try_send(EngineEvent::Trade(Trade {
                    long_id: order.user_id.clone(),
                    short_id: ask.user_id.clone(),
                    long_leverage: order.leverage,
                    short_leverage: ask.leverage,
                    amount: trade_amount,
                    price,
                }));

                if ask.amount == dec!(0) {
                    queue.pop_front();
                }
                if order.amount == dec!(0) {
                    break;
                }
            }

            if queue.is_empty() {
                prices_to_remove.push(price);
            }

            if order.amount == dec!(0) {
                break;
            }
        }

        for price in prices_to_remove {
            self.asks.remove(&price);
        }

        let dynamic_status = |type_: OrderType| match type_ {
            MARKET => "disreguarding remaining amount.".to_string(),
            LIMIT => "adding remaining in queue.".to_string(),
        };

        if order.amount > dec!(0) {
            if let Some(responder) = order.responder.take() {
                let _ = responder.send(OrderResponse {
                    status: dynamic_status(order.order_type.clone()),
                    filled,
                    remaining: order.amount,
                });
            }
            if order.order_type == LIMIT {
                self.bids.entry(order.price).or_default().push_back(order);
            }
        } else {
            if let Some(responder) = order.responder.take() {
                let _ = responder.send(OrderResponse {
                    status: "order completely filled".to_string(),
                    filled,
                    remaining: dec!(0),
                });
            }
        }
    }

    pub fn handle_sell(&mut self, mut order: Order) {
        let mut filled = dec!(0);
        let mut prices_to_remove: Vec<Price> = Vec::new();

        // descending price order for matching with best bids
        for (&price, queue) in self.bids.iter_mut().rev() {
            if order.order_type == LIMIT && price < order.price {
                match self.asks.get_mut(&order.price) {
                    Some(sells) => {
                        if let Some(responder) = order.responder.take() {
                            let _ = responder.send(OrderResponse {
                                status: "order partially filled, remaining added to queue!"
                                    .to_string(),
                                filled,
                                remaining: order.amount,
                            });
                        }
                        sells.push_back(order);
                        return;
                    }
                    None => {
                        let mut new_queue: VecDeque<Order> = VecDeque::new();

                        if let Some(responder_tx) = order.responder.take() {
                            responder_tx.send(OrderResponse {
                                status: "could not match, adding to queue".to_string(),
                                filled: filled,
                                remaining: order.amount,
                            });
                        }

                        new_queue.push_back(order);
                        self.asks.insert(price, new_queue);
                        return;
                    }
                }
            }

            while let Some(bid) = queue.front_mut() {
                let trade_amount = order.amount.min(bid.amount);
                println!(
                    "Matched SELL {} with BUY {} @ {} for {}",
                    order.id, bid.id, price, trade_amount
                );

                order.amount -= trade_amount;
                bid.amount -= trade_amount;
                filled += trade_amount;

                // let the position tracker know the trade just happened here
                if let Err(e) = self.position_tx.try_send(EngineEvent::Trade(Trade {
                    long_id: order.user_id.clone(),
                    short_id: bid.user_id.clone(),
                    long_leverage: order.leverage,
                    short_leverage: bid.leverage,
                    amount: trade_amount,
                    price,
                })) {
                    println!("[ERROR] {}", e);
                }

                if bid.amount == dec!(0) {
                    queue.pop_front();
                }
                if order.amount == dec!(0) {
                    break;
                }
            }

            if queue.is_empty() {
                prices_to_remove.push(price);
            }

            if order.amount == dec!(0) {
                break;
            }
        }

        for price in prices_to_remove {
            self.bids.remove(&price);
        }

        let dynamic_status = |order_type: OrderType| match order_type {
            MARKET => "disregarding remaining amount.".to_string(),
            LIMIT => "adding remaining in queue.".to_string(),
        };

        if order.amount > dec!(0) {
            if let Some(responder) = order.responder.take() {
                let _ = responder.send(OrderResponse {
                    status: dynamic_status(order.order_type.clone()),
                    filled,
                    remaining: order.amount,
                });
            }
            if order.order_type == LIMIT {
                self.asks.entry(order.price).or_default().push_back(order);
            }
        } else {
            if let Some(responder) = order.responder.take() {
                let _ = responder.send(OrderResponse {
                    status: "order completely filled".to_string(),
                    filled,
                    remaining: dec!(0),
                });
            }
        }
    }

    pub fn update_best_prices(&mut self) {
        self.best_bid = self.bids.keys().next_back().copied();
        self.best_ask = self.asks.keys().next().copied();
    }

    // // Helper methods for debugging and monitoring
    // pub fn get_spread(&self) -> Option<u64> {
    //     match (self.best_buy, self.best_sell) {
    //         (Some(bid), Some(ask)) => Some(ask - bid),
    //         _ => None,
    //     }
    // }
    //
    // pub fn get_book_depth(&self, levels: usize) -> (Vec<(u64, u64)>, Vec<(u64, u64)>) {
    //     let bids: Vec<(u64, u64)> = self
    //         .buys
    //         .iter()
    //         .rev()
    //         .take(levels)
    //         .map(|(&price, queue)| (price, queue.iter().map(|o| o.amount).sum()))
    //         .collect();
    //
    //     let asks: Vec<(u64, u64)> = self
    //         .sells
    //         .iter()
    //         .take(levels)
    //         .map(|(&price, queue)| (price, queue.iter().map(|o| o.amount).sum()))
    //         .collect();
    //
    //     (bids, asks)
    // }
}
