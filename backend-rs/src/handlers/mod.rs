pub mod order;
pub mod websocket;

pub use order::order_handler;
pub use websocket::{broadcast_trade, ws_handler};

use crate::types::Response;
use axum::response::Json;

pub async fn handler() -> Json<Response> {
    Json(Response {
        message: "Hello, friend".to_string(),
        error: "".to_string(),
    })
}
