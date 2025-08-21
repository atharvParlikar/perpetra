mod domain;
mod handlers;
mod state;

mod types;
use axum::{routing::any, routing::get, routing::post, Router};
use crossbeam::atomic::AtomicCell;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

use domain::order::OrderBook;
use domain::position::EngineEvent;
use domain::position::PositionTracker;
use handlers::{broadcast_trade, handler, order_handler, ws_handler};
use state::BookState;

use domain::Oracle;

use domain::position::run_position_loop;
use handlers::websocket::SocketList;

use crate::types::OrderBookMessage;

// Add any missing imports that were in your original main.rs

#[tokio::main]
async fn main() {
    let (book_tx, mut book_rx) = mpsc::channel::<OrderBookMessage>(10000);

    let (liquidation_order_queue_tx, mut liquidation_order_queue_rx) =
        mpsc::channel::<OrderBookMessage>(10000);

    let (position_tx, position_rx) = mpsc::unbounded_channel::<EngineEvent>();

    let (oracle_tx, oracle_rx) = mpsc::unbounded_channel::<Decimal>();

    let mut book = OrderBook::new(position_tx.clone());
    let positions = PositionTracker::new(liquidation_order_queue_tx);
    let sockets: Arc<Mutex<SocketList>> = Arc::new(Mutex::new(Vec::new()));

    let book_state = BookState {
        tx: book_tx.clone(),
    };

    let app: Router = Router::new()
        .route("/", get(handler))
        .route("/order", post(order_handler))
        .with_state(book_state.clone())
        .route("/ws", any(ws_handler))
        .with_state(sockets.clone());

    // Orderbook thread
    std::thread::spawn(move || {
        let mini_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime on book thread");

        mini_runtime.block_on(async move {
            loop {
                while let Ok(message) = liquidation_order_queue_rx.try_recv() {
                    match message {
                        OrderBookMessage::Order(order) => {
                            println!("[LIQUIDATION] order: {}", order);
                            book.insert_order(order);
                        }
                    }
                }

                match book_rx.recv().await {
                    Some(OrderBookMessage::Order(order)) => {
                        println!("[ORDER] {}", order);
                        book.insert_order(order);
                    }
                    None => {
                        if liquidation_order_queue_rx.is_closed() {
                            eprintln!("LIQUIDATION QUEUE IS CLOSED, SOMETHING FUCKED, CHECK IT");
                            break;
                        }
                    }
                }
            }
        });
    });

    let mark_price = Arc::new(AtomicCell::new(Decimal::ZERO));

    // Position tracker thread
    std::thread::spawn(move || {
        let mini_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime on book thread");

        mini_runtime.block_on(async move {
            run_position_loop(oracle_rx, position_rx, positions, sockets).await;
        });
    });

    // Oracle thread
    tokio::spawn({
        async move {
            let mut interval = interval(Duration::from_millis(500));
            let mut oracle = Oracle::new(None);
            loop {
                interval.tick().await;
                let price = oracle.next_price().price_usd;
                mark_price.store(price);
                if let Err(error) = oracle_tx.send(price) {
                    //  TODO: maybe add a logging system instead of just printing error to terminal
                    eprintln!("[ORACLE ERROR] {}", error);
                }
            }
        }
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
