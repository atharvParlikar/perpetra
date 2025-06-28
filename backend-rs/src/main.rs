#![allow(unused)]

mod order;

use std::sync::Arc;

use order::{
    Order, OrderBook, OrderType,
    OrderType::{LIMIT, MARKET},
};

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

#[derive(Default)]
struct DebugMsg {
    message: String,
    pub responder: Option<oneshot::Sender<String>>,
}

#[derive(Default)]
struct Message {
    order: Option<Order>,
    debug: Option<DebugMsg>,
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
    amount: u64,
    price: u64,
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

    let order = Order {
        id: Uuid::new_v4().to_string(),
        user_id: payload.jwt,
        order_type: type_,
        amount: payload.amount,
        price: payload.price,
        side: payload.side,
        responder: Some(resp_tx),
    };

    if let Err(e) = state
        .tx
        .send(Message {
            order: Some(order),
            ..Message::default()
        })
        .await
    {
        return Json(Response {
            message: "".to_string(),
            error: format!("Failed to send order to processing thread:\n{}", e),
        });
    }

    match resp_rx.await {
        Ok(response) => Json(Response {
            message: format!(
                "Order processed: filled {}, remaining {}",
                response.filled, response.remaining
            ),
            error: String::from(""),
        }),
        Err(_) => Json(Response {
            message: String::from(""),
            error: "Order was dropped before response".to_string(),
        }),
    }
}

async fn debug_handler(State(state): State<AppState>) -> Json<Response> {
    let (resp_tx, resp_rx) = oneshot::channel::<String>();

    if let Err(e) = state
        .tx
        .send(Message {
            debug: Some(DebugMsg {
                message: "book".to_string(),
                responder: Some(resp_tx),
            }),
            ..Message::default()
        })
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
    let (tx, mut rx) = mpsc::channel::<Message>(10000);
    let tx = Arc::new(tx);

    let mut book = OrderBook::new();

    let state = AppState { tx: tx.clone() };

    let app: Router = Router::new()
        .route("/", get(handler))
        .route("/order", post(order_handler))
        .with_state(state.clone())
        .route("/debug", get(debug_handler))
        .with_state(state);

    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            match message.order {
                Some(order) => {
                    println!("Recived order: {}", order);
                    book.insert_order(order);
                }
                None => {}
            }
            match message.debug {
                Some(dbg_msg) => {
                    if let Some(responder) = dbg_msg.responder {
                        responder.send(format!("{}", book));
                        println!("{}", book);
                    }
                }
                None => {}
            }
        }
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
