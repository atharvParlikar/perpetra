use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use tokio::sync::oneshot;
use OrderType::{LIMIT, MARKET};

#[derive(Debug, Clone, PartialEq)]
pub enum OrderType {
    MARKET,
    LIMIT,
}

pub struct Order {
    pub id: String,
    pub user_id: String,
    pub order_type: OrderType,
    pub amount: u64,
    pub price: u64,
    pub side: String,

    pub responder: Option<oneshot::Sender<OrderResponse>>,
}

#[derive(Default)]
pub struct OrderBook {
    pub buys: BTreeMap<u64, VecDeque<Order>>,
    pub sells: BTreeMap<u64, VecDeque<Order>>,
    pub best_buy: Option<u64>,
    pub best_sell: Option<u64>,
}

#[derive(Clone)]
pub struct OrderResponse {
    pub status: String,
    pub filled: u64,
    pub remaining: u64,
}

impl fmt::Display for Order {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}\n--------\nprice: {}\namount: {}\nside: {}\nuser: {}",
            self.id, self.price, self.amount, self.side, self.user_id
        )
    }
}

impl fmt::Display for OrderBook {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.buys.is_empty() && self.sells.is_empty() {
            return write!(f, "OrderBook is empty");
        }

        let mut output = String::new();

        // Process bids
        if !self.buys.is_empty() {
            output.push_str("=== BIDS ===\n");
            for (price, orders) in &self.buys {
                output.push_str(&format!("Price level: {}\n", price));
                for order in orders {
                    output.push_str(&format!("{}\n", order));
                }
                output.push_str("\n");
            }
        }

        // Process asks
        if !self.sells.is_empty() {
            output.push_str("=== ASKS ===\n\n");
            for (price, orders) in &self.sells {
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_order(&mut self, order: Order) {
        match order.side.as_str() {
            "buy" => self.handle_buy(order),
            "sell" => self.handle_sell(order),
            _ => println!("Invalid side: {}", order.side),
        }
        self.update_best_prices();
    }

    pub fn handle_buy(&mut self, mut order: Order) {
        let mut filled = 0;

        let mut prices_to_remove: Vec<u64> = Vec::new();

        // ascending price order
        for (&price, queue) in self.sells.iter_mut() {
            if order.order_type == LIMIT && price > order.price {
                match self.buys.get_mut(&order.price) {
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
                        new_queue.push_back(order);
                        self.buys.insert(price, new_queue);
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

                if ask.amount == 0 {
                    queue.pop_front();
                }
                if order.amount == 0 {
                    break;
                }
            }

            if queue.is_empty() {
                prices_to_remove.push(price);
            }

            if order.amount == 0 {
                break;
            }
        }

        for price in prices_to_remove {
            self.sells.remove(&price);
        }

        let dynamic_status = |type_: OrderType| match type_ {
            MARKET => "order partially filled, disreguarding remaining amount.".to_string(),
            LIMIT => "order partially filled, adding remaining in queue.".to_string(),
        };

        if order.amount > 0 {
            if let Some(responder) = order.responder.take() {
                let _ = responder.send(OrderResponse {
                    status: dynamic_status(order.order_type.clone()),
                    filled,
                    remaining: order.amount,
                });
            }
            if order.order_type == LIMIT {
                self.buys.entry(order.price).or_default().push_back(order);
            }
        } else {
            if let Some(responder) = order.responder.take() {
                let _ = responder.send(OrderResponse {
                    status: "order completely filled".to_string(),
                    filled,
                    remaining: 0,
                });
            }
        }
    }

    pub fn handle_sell(&mut self, mut order: Order) {
        let mut filled = 0;
        let mut prices_to_remove: Vec<u64> = Vec::new();

        // descending price order for matching with best bids
        for (&price, queue) in self.buys.iter_mut().rev() {
            if order.order_type == LIMIT && price < order.price {
                match self.sells.get_mut(&order.price) {
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
                        new_queue.push_back(order);
                        self.sells.insert(price, new_queue);
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

                if bid.amount == 0 {
                    queue.pop_front();
                }
                if order.amount == 0 {
                    break;
                }
            }

            if queue.is_empty() {
                prices_to_remove.push(price);
            }

            if order.amount == 0 {
                break;
            }
        }

        for price in prices_to_remove {
            self.buys.remove(&price);
        }

        let dynamic_status = |order_type: OrderType| match order_type {
            MARKET => "order partially filled, disregarding remaining amount.".to_string(),
            LIMIT => "order partially filled, adding remaining in queue.".to_string(),
        };

        if order.amount > 0 {
            if let Some(responder) = order.responder.take() {
                let _ = responder.send(OrderResponse {
                    status: dynamic_status(order.order_type.clone()),
                    filled,
                    remaining: order.amount,
                });
            }
            if order.order_type == LIMIT {
                self.sells.entry(order.price).or_default().push_back(order);
            }
        } else {
            if let Some(responder) = order.responder.take() {
                let _ = responder.send(OrderResponse {
                    status: "order completely filled".to_string(),
                    filled,
                    remaining: 0,
                });
            }
        }
    }

    pub fn update_best_prices(&mut self) {
        self.best_buy = self.buys.keys().next_back().copied();
        self.best_sell = self.sells.keys().next().copied();
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
