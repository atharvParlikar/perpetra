mod order;
use order::{Order, OrderBook};

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    tx: mpsc::Sender<Order>,
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

    let order = Order {
        id: Uuid::new_v4().to_string(),
        user_id: payload.jwt,
        type_: payload.type_,
        amount: payload.amount,
        price: payload.price,
        side: payload.side,
        responder: Some(resp_tx),
    };

    state.tx.send(order).await.unwrap();

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

#[tokio::main]
async fn main() {
    let (tx, mut rx) = mpsc::channel::<Order>(10000);
    let mut book = OrderBook::new();

    let app: Router = Router::new()
        .route("/", get(handler))
        .route("/order", post(order_handler))
        .with_state(AppState { tx });

    tokio::spawn(async move {
        while let Some(order) = rx.recv().await {
            println!("Recived order: {:?}", order);
            book.insert_order(order);
        }
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
