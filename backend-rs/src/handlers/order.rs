use axum::{extract::State, http::StatusCode, response::IntoResponse, response::Json};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::domain::{Order, OrderType, Side};
use crate::state::BookState;
use crate::types::{OrderBookMessage, OrderRequest, Response};

pub async fn order_handler(
    State(state): State<BookState>,
    Json(payload): Json<OrderRequest>,
) -> impl IntoResponse {
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();

    let type_ = match payload.type_.as_str() {
        "limit" => OrderType::LIMIT,
        "market" => OrderType::MARKET,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(Response {
                    message: String::new(),
                    error: format!("Invalid order type: {}", other),
                }),
            );
        }
    };

    let side = match payload.side.as_str() {
        "buy" => Side::BID,
        "sell" => Side::ASK,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(Response {
                    message: String::new(),
                    error: format!("Invalid side: {}", other),
                }),
            );
        }
    };

    let (price, amount) = match (
        Decimal::from_f64(payload.price),
        Decimal::from_f64(payload.amount),
    ) {
        (Some(p), Some(a)) => (p, a),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(Response {
                    message: String::new(),
                    error: format!(
                        "Invalid price or amount: {} / {}",
                        payload.price, payload.amount
                    ),
                }),
            );
        }
    };

    let leverage = Decimal::from_u32(payload.leverage).unwrap_or(dec!(1));

    let order = Order {
        id: Uuid::new_v4().to_string(),
        user_id: payload.jwt,
        order_type: type_,
        amount,
        price,
        side,
        leverage,
        responder: Some(resp_tx),
    };

    if let Err(e) = state.tx.send(OrderBookMessage::Order(order)).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response {
                message: String::new(),
                error: format!("Failed to send order to processing thread: {}", e),
            }),
        );
    }

    match resp_rx.await {
        Ok(response) => (
            StatusCode::OK,
            Json(Response {
                message: format!(
                    "Order processed: filled {}, remaining {}, {}",
                    response.filled, response.remaining, response.status
                ),
                error: String::new(),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response {
                message: String::new(),
                error: format!("Order was dropped before response: {}", e),
            }),
        ),
    }
}
