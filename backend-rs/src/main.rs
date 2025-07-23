#![allow(unused)]

mod oracle;
mod order;
mod position;

use std::{
    fmt::{format, Debug},
    sync::Arc,
};

use crossbeam::{atomic::AtomicCell, channel::tick, select, thread};
use oracle::Oracle;
use order::{
    Order, OrderBook,
    OrderType::{self, LIMIT, MARKET},
    Side,
};

use axum::{
    extract::ws,
    extract::ws::{WebSocket, WebSocketUpgrade},
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{any, get, post},
    Json, Router,
};
use position::{EngineEvent, Position, PositionMap, PositionTracker};
use rust_decimal::{
    prelude::{FromPrimitive, Zero},
    Decimal,
};
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot, Mutex},
    time::{interval, Duration},
};
use uuid::Uuid;

use crate::position::Trade;

type SocketList = Vec<mpsc::Sender<String>>;

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
pub struct BookState {
    tx: mpsc::Sender<Message>,
}

#[derive(Clone)]
pub struct PositionsState {
    tx: Arc<crossbeam::channel::Sender<Message>>,
}

#[derive(Serialize)]
struct Response {
    message: String,
    error: String,
}

#[derive(Deserialize)]
pub struct OrderRequest {
    type_: String,
    amount: f64,
    price: f64,
    side: String,
    leverage: u32,
    jwt: String, // TODO
}

async fn handler() -> Json<Response> {
    Json(Response {
        message: "Hello, friend".to_string(),
        error: "".to_string(),
    })
}

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

    if let Err(e) = state.tx.send(Message::Order(order)).await {
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

async fn debug_handler(State(state): State<BookState>) -> Json<Response> {
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

async fn ws_handler(
    State(sockets): State<Arc<Mutex<SocketList>>>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    let sockets = sockets.clone();
    ws.on_upgrade(|socket| handle_socket(socket, sockets))
}

async fn handle_socket(mut socket: WebSocket, sockets: Arc<Mutex<SocketList>>) {
    while let Some(msg) = socket.recv().await {
        let msg = if let Ok(msg) = msg {
            msg
        } else {
            return;
        };

        if socket.send(msg).await.is_err() {
            return;
        }
    }

    let (socket_tx, mut socket_rx) = mpsc::channel::<String>(1000);

    {
        let mut socket_list = sockets.lock().await;
        socket_list.push(socket_tx);
    }

    while let Some(msg) = socket_rx.recv().await {
        socket.send(ws::Message::text(msg));
    }
}

async fn broadcast_order(order: Order, sockets: Arc<Mutex<SocketList>>) {
    let socket_list = sockets.lock().await;
    for socket_sender in socket_list.iter() {
        socket_sender.send(format!("{}", order));
    }
}

async fn broadcast_trade(trade: &Trade, sockets: Arc<Mutex<SocketList>>) {
    println!("trade brodcasted: {}", trade);
    let socket_list = sockets.lock().await;
    for socket_sender in socket_list.iter() {
        socket_sender.send(format!("{}", trade));
    }
}

#[tokio::main]
async fn main() {
    let (book_tx, mut book_rx) = mpsc::channel::<Message>(10000);

    let (liquidation_order_queue_tx, mut liquidation_order_queue_rx) =
        mpsc::channel::<Message>(10000);

    let (position_tx, mut position_rx) = crossbeam::channel::unbounded::<EngineEvent>();

    let (oracle_tx, mut oracle_rx) = crossbeam::channel::unbounded::<Decimal>();

    let mut book = OrderBook::new(position_tx.clone());
    let mut positions = PositionTracker::new(liquidation_order_queue_tx);
    let mut sockets: Arc<Mutex<SocketList>> = Arc::new(Mutex::new(Vec::new()));

    let book_state = BookState {
        tx: book_tx.clone(),
    };

    let app: Router = Router::new()
        .route("/", get(handler))
        .route("/order", post(order_handler))
        .with_state(book_state.clone())
        .route("/debug", get(debug_handler))
        .with_state(book_state.clone())
        .route("/ws", any(ws_handler))
        .with_state(sockets.clone());

    let book_thread = std::thread::spawn(move || {
        let mini_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create runtime");

        mini_runtime.block_on(async move {
            loop {
                tokio::select! {
                    biased;

                    Some(message) = liquidation_order_queue_rx.recv() => {
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
                                println!("[ORDER] {}", order);
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

    let mark_price = Arc::new(AtomicCell::new(Decimal::ZERO));

    let position_thread = std::thread::spawn(move || {
        crossbeam::select! {
            recv(oracle_rx) -> mark_price_maybe => {
                match mark_price_maybe {
                    Ok(mark_price) => positions.update_risk(mark_price),
                    Err(e) => {} // TODO
                }
            }
            recv(position_rx) -> position_maybe => {
                match position_maybe {
                    Ok(EngineEvent::Trade(trade)) => {
                        positions.update_position(&trade);
                        broadcast_trade(&trade, sockets);
                    },
                    Err(_) => {} // TODO
                }
            }
        }
    });

    tokio::spawn({
        async move {
            let mut interval = interval(Duration::from_millis(500));
            let mut oracle = Oracle::new(None);
            loop {
                interval.tick().await;
                let price = oracle.next_price().price_usd;
                mark_price.store(price);
            }
        }
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
