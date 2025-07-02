#![allow(unused)]

mod oracle;
mod order;
mod position;

use std::{fmt::format, sync::Arc};

use oracle::Oracle;
use order::{
    Order, OrderBook,
    OrderType::{self, LIMIT, MARKET},
    Side,
};

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use position::{EngineEvent, Position, PositionMap, PositionTracker};
use rust_decimal::{prelude::FromPrimitive, Decimal};
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot},
    time::{interval, Duration},
};
use uuid::Uuid;

#[derive(Default)]
struct DebugMsg {
    message: String,
    pub responder: Option<oneshot::Sender<String>>,
}

enum Message {
    Order(Order),
    Debug(DebugMsg),
}

#[derive(Clone)]
struct AppState {
    tx: Arc<mpsc::Sender<Message>>,
}

#[derive(Serialize)]
struct Response {
    message: String,
    error: String,
}

#[derive(Deserialize)]
struct OrderRequest {
    type_: String,
    amount: f64,
    price: f64,
    side: String,
    jwt: String, // TODO
}

async fn handler() -> Json<Response> {
    Json(Response {
        message: "Hello, friend".to_string(),
        error: "".to_string(),
    })
}

async fn order_handler(
    State(state): State<AppState>,
    Json(payload): Json<OrderRequest>,
) -> Json<Response> {
    let (resp_tx, resp_rx) = oneshot::channel();

    let mut type_: OrderType = MARKET;

    if payload.type_ == "limit" {
        type_ = LIMIT;
    }

    let side = match payload.side.as_str() {
        "buy" => Side::BID,
        "sell" => Side::ASK,
        _ => {
            return Json(Response {
                message: String::new(),
                error: format!("Invalid side: {}", payload.side),
            })
        }
    };

    let (price, amount) = match (
        Decimal::from_f64(payload.price),
        Decimal::from_f64(payload.amount),
    ) {
        (Some(p), Some(a)) => (p, a),
        _ => {
            return Json(Response {
                message: String::new(),
                error: format!(
                    "Invalid price / amount: {} / {}",
                    payload.price, payload.amount
                ),
            });
        }
    };

    let order = Order {
        id: Uuid::new_v4().to_string(),
        user_id: payload.jwt,
        order_type: type_,
        amount,
        price,
        side,
        responder: Some(resp_tx),
    };

    if let Err(e) = state.tx.send(Message::Order(order)).await {
        return Json(Response {
            message: "".to_string(),
            error: format!("Failed to send order to processing thread:\n{}", e),
        });
    }

    match resp_rx.await {
        Ok(response) => Json(Response {
            message: format!(
                "Order processed: filled {}, remaining {}, {}",
                response.filled, response.remaining, response.status
            ),
            error: String::from(""),
        }),
        Err(e) => Json(Response {
            message: String::from(""),
            error: format!("Order was dropped before response | {}", e),
        }),
    }
}

async fn debug_handler(State(state): State<AppState>) -> Json<Response> {
    let (resp_tx, resp_rx) = oneshot::channel::<String>();

    if let Err(e) = state
        .tx
        .send(Message::Debug(DebugMsg {
            message: "book".to_string(),
            responder: Some(resp_tx),
        }))
        .await
    {
        return Json(Response {
            message: "".to_string(),
            error: format!("Cannot send debug message to the orderbook thread:\n{}", e),
        });
    }

    match resp_rx.await {
        Ok(response) => Json(Response {
            message: response,
            error: "".to_string(),
        }),
        Err(e) => Json(Response {
            message: "".to_string(),
            error: format!("Something wen't wrong reciving the message:\n{}", e),
        }),
    }
}

#[tokio::main]
async fn main() {
    let (book_tx, mut book_rx) = mpsc::channel::<Message>(10000);
    let book_tx = Arc::new(book_tx);

    let (book_liquidation_tx, mut book_liquidation_rx) = mpsc::channel::<Message>(10000);
    let book_liquidation_tx = Arc::new(book_liquidation_tx);

    let (position_tx, mut position_rx) = mpsc::channel::<EngineEvent>(10000);
    let position_tx = Arc::new(position_tx);

    let mut book = OrderBook::new(position_tx.clone());
    let mut positions = PositionTracker::new(book_liquidation_tx);

    let state = AppState {
        tx: book_tx.clone(),
    };

    let app: Router = Router::new()
        .route("/", get(handler))
        .route("/order", post(order_handler))
        .with_state(state.clone())
        .route("/debug", get(debug_handler))
        .with_state(state);

    let book_thread = std::thread::spawn(move || {
        let mini_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create runtime");

        mini_runtime.block_on(async move {
            loop {
                tokio::select! {
                    Some(message) = book_liquidation_rx.recv() => {
                        match message {
                            Message::Order(order) => {
                                println!("[LIQUIDATION] order: {}", order);
                                book.insert_order(order);
                            }
                            _ => println!("Unexpected liquidation message"),
                        }
                    }

                    Some(message) = book_rx.recv() => {
                        match message {
                            Message::Order(order) => {
                                println!("📘 Normal order: {}", order);
                                book.insert_order(order);
                            }
                            Message::Debug(dbg_msg) => {
                                if let Some(responder) = dbg_msg.responder {
                                    responder.send(format!("{}", book));
                                }
                            }
                        }
                    }

                    else => break, // All senders closed
                }
            }
        });
    });

    let position_thread = std::thread::spawn(move || {
        while let Some(event) = position_rx.blocking_recv() {
            match event {
                EngineEvent::Trade(trade) => positions.update_position(trade),
                EngineEvent::Oracle { mark_price } => positions.update_risk(mark_price),
            }
        }
    });

    let mut interval = interval(Duration::from_millis(1000));
    let mut oracle = Oracle::new(None);

    loop {
        interval.tick().await;
        let mark_price = oracle.next_price().price_usd;
        println!("Mark price: {}", mark_price);
        position_tx.send(EngineEvent::Oracle { mark_price });
    }

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
