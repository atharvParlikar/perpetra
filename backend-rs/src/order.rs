use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use tokio::sync::oneshot;

#[derive(Debug)]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub type_: String,
    pub amount: u64,
    pub price: u64,
    pub side: String,

    pub responder: Option<oneshot::Sender<OrderResponse>>,
}

#[derive(Default)]
pub struct OrderBook {
    pub bids: BTreeMap<u64, VecDeque<Order>>,
    pub asks: BTreeMap<u64, VecDeque<Order>>,
    pub best_bid: Option<u64>,
    pub best_ask: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct OrderResponse {
    pub status: String,
    pub filled: u64,
    pub remaining: u64,
}

impl fmt::Display for Order {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}--------\nprice{}\namount{}\nside:{}\nuser:{}",
            self.id, self.price, self.amount, self.side, self.user_id
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
                    output.push_str(&format!("  {}\n", order));
                }
                output.push_str("\n");
            }
        }

        // Process asks
        if !self.asks.is_empty() {
            output.push_str("=== ASKS ===\n");
            for (price, orders) in &self.asks {
                output.push_str(&format!("Price level: {}\n", price));
                for order in orders.iter().rev() {
                    output.push_str(&format!("  {}\n", order));
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
        // Market orders or limit orders that can match
        if order.type_ == "market" {
            let mut prices_to_remove: Vec<u64> = Vec::new();

            // ascending price order
            for (&price, queue) in self.asks.iter_mut() {
                while let Some(ask) = queue.front_mut() {
                    let trade_amount = order.amount.min(ask.amount);
                    println!(
                        "Matched BUY {} with SELL {} @ {} for {}",
                        order.id, ask.id, price, trade_amount
                    );

                    order.amount -= trade_amount;
                    ask.amount -= trade_amount;

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
                self.asks.remove(&price);
            }
        } else { // limit order
        }

        if order.amount > 0 {
            self.bids.entry(order.price).or_default().push_back(order);
        }
    }

    pub fn handle_sell(&mut self, mut order: Order) {
        // Market orders or limit orders that can match
        if order.type_ == "market" || self.best_bid.map_or(false, |p| order.price <= p) {
            let mut prices_to_remove: Vec<u64> = Vec::new();

            println!("==== ITERATION ====");

            // Iterate bids in descending price order (best execution)
            for (&price, queue) in self.bids.iter_mut().rev() {
                if order.type_ == "limit" && price < order.price {
                    break;
                }

                while let Some(bid) = queue.front_mut() {
                    println!("BID: {:?}", order);
                    let trade_amount = order.amount.min(bid.amount);
                    println!(
                        "Matched SELL {} with BUY {} @ {} for {}",
                        order.id, bid.id, price, trade_amount
                    );

                    order.amount -= trade_amount;
                    bid.amount -= trade_amount;

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

            println!("==== END ITERATION ====");

            // Clean up empty price levels
            for price in prices_to_remove {
                self.bids.remove(&price);
            }
        }

        // Add remaining amount as limit order
        if order.amount > 0 && order.type_ == "limit" {
            self.asks.entry(order.price).or_default().push_back(order);
        }
    }

    pub fn update_best_prices(&mut self) {
        self.best_bid = self.bids.keys().next_back().copied();
        self.best_ask = self.asks.keys().next().copied();
    }

    // Helper methods for debugging and monitoring
    pub fn get_spread(&self) -> Option<u64> {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn get_book_depth(&self, levels: usize) -> (Vec<(u64, u64)>, Vec<(u64, u64)>) {
        let bids: Vec<(u64, u64)> = self
            .bids
            .iter()
            .rev()
            .take(levels)
            .map(|(&price, queue)| (price, queue.iter().map(|o| o.amount).sum()))
            .collect();

        let asks: Vec<(u64, u64)> = self
            .asks
            .iter()
            .take(levels)
            .map(|(&price, queue)| (price, queue.iter().map(|o| o.amount).sum()))
            .collect();

        (bids, asks)
    }
}
