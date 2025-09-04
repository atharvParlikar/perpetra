mod domain;
mod handlers;
mod state;

mod types;
use axum::{routing::any, routing::get, routing::post, Router};
use std::collections::HashMap;
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

use crate::domain::oracle::BtcPrice;
use crate::domain::wallet::WalletEvent;
use crate::domain::wallet::WalletManager;
use crate::domain::wallet::WalletOneshotReply;
use crate::types::OrderBookMessage;

// Add any missing imports that were in your original main.rs

#[tokio::main]
async fn main() {
    let (book_tx, mut book_rx) = mpsc::channel::<OrderBookMessage>(10000);

    let (liquidation_order_queue_tx, mut liquidation_order_queue_rx) =
        mpsc::channel::<OrderBookMessage>(10000);

    let (position_tx, position_rx) = mpsc::unbounded_channel::<EngineEvent>();

    let (oracle_tx, oracle_rx) = mpsc::unbounded_channel::<BtcPrice>();

    let mut wallets = WalletManager::new();
    let (wallet_tx, mut wallet_rx) = mpsc::unbounded_channel::<WalletEvent>();

    let mut book = OrderBook::new(position_tx.clone(), wallet_tx.clone());
    let positions = PositionTracker::new(liquidation_order_queue_tx, wallet_tx.clone());
    let sockets: Arc<Mutex<SocketList>> = Arc::new(Mutex::new(HashMap::new()));

    let book_state = BookState { tx: book_tx };

    let app: Router = Router::new()
        .route("/", get(handler))
        .route("/order", post(order_handler))
        .with_state(book_state)
        .route("/ws", any(ws_handler))
        .with_state(sockets.clone());

    // Orderbook thread
    std::thread::spawn(move || {
        let mini_runtime = tokio::runtime::Builder::new_current_thread()
            .thread_name("orderbook thread")
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime on book thread");

        mini_runtime.block_on(async move {
            loop {
                tokio::select! {
                    biased;

                    maybe_liquidation_message = liquidation_order_queue_rx.recv() => {
                        if let Some(OrderBookMessage::Order(order)) = maybe_liquidation_message {
                            println!("[LIQUIDATION] order: {}", order);
                            book.insert_order(order).await;
                        }
                    }

                    maybe_order_message = book_rx.recv() => {
                        if let Some(OrderBookMessage::Order(order)) = maybe_order_message {
                            println!("[ORDER] {}", order);
                            book.insert_order(order).await;
                        }
                    }
                }
            }
        });
    });

    // Position tracker thread
    std::thread::spawn(move || {
        let mini_runtime = tokio::runtime::Builder::new_current_thread()
            .thread_name("position tracker thread")
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime on positions thread");

        mini_runtime.block_on(async move {
            run_position_loop(oracle_rx, position_rx, positions, sockets).await;
        });
    });

    // wallet thread
    std::thread::spawn(move || {
        let mini_runtime = tokio::runtime::Builder::new_current_thread()
            .thread_name("wallet thread")
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime on book thread");

        mini_runtime.block_on(async move {
            while let Some(message) = wallet_rx.recv().await {
                match message {
                    WalletEvent::Debit(message) => {
                        let debit_success = wallets.debit(message.wallet_id, message.amount);
                        let oneshot_reply_message = if debit_success {
                            "".to_string()
                        } else {
                            "insufficient balance".to_string()
                        };

                        if let Some(sender) = message.oneshot_reply {
                            let sent = sender.send(WalletOneshotReply {
                                success: debit_success,
                                message: oneshot_reply_message,
                            });

                            if let Err(_) = sent {
                                println!("[WALLET THREAD ERROR] can't send oneshot reply");
                            }
                        }
                    }
                    WalletEvent::Credit(message) => {
                        wallets.credit(message.wallet_id, message.amount)
                    }
                }
            }
        });
    });

    // Oracle thread
    tokio::spawn({
        async move {
            let mut interval = interval(Duration::from_millis(500));
            let mut oracle = Oracle::new(None);
            loop {
                interval.tick().await;
                let price = oracle.next_price();
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
