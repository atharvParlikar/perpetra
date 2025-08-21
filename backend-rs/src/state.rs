use tokio::sync::mpsc;

use crate::types::OrderBookMessage;

#[derive(Clone)]
pub struct BookState {
    pub tx: mpsc::Sender<OrderBookMessage>,
}
