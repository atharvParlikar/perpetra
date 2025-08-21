use serde::{Deserialize, Serialize};

use crate::domain::{position::Trade, Order};

#[derive(Serialize)]
pub struct Response {
    pub message: String,
    pub error: String,
}

#[derive(Deserialize)]
pub struct OrderRequest {
    pub type_: String,
    pub amount: f64,
    pub price: f64,
    pub side: String,
    pub leverage: u32,
    pub jwt: String, // TODO
}

pub enum OrderBookMessage {
    Order(Order),
}

pub enum SocketMessage {
    Trade(Trade),
}
