use axum::extract::ws::{self, WebSocket};
use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response as AxumResponse,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::domain::position::Trade;
use crate::types::{SocketMessageRecv, SocketMessageSend};

pub type SocketList = HashMap<String, mpsc::Sender<SocketMessageSend>>;

pub async fn ws_handler(
    State(sockets): State<Arc<Mutex<SocketList>>>,
    ws: WebSocketUpgrade,
) -> AxumResponse {
    ws.on_upgrade(|socket| handle_socket(socket, sockets))
}

pub async fn handle_socket(mut socket: WebSocket, sockets: Arc<Mutex<SocketList>>) {
    println!("Some ws client connected");

    let (socket_tx, mut socket_rx) = mpsc::channel::<SocketMessageSend>(1000);

    loop {
        if let Ok(jwt) = handle_websocket_message(&mut socket).await {
            let mut socket_list = sockets.lock().await;
            socket_list.insert(jwt, socket_tx);
            println!("socket_list len: {}", socket_list.len());
            break;
        }
    }

    loop {
        if let Some(msg) = socket_rx.recv().await {
            match msg {
                SocketMessageSend::Trade(trade) => {
                    if let Ok(json) = serde_json::to_string(&trade) {
                        if let Err(error) = socket.send(ws::Message::text(json)).await {
                            eprintln!("[SOCKET ERROR]:\n{}", error);
                        }
                    }
                }
            }
        }
    }
}

async fn handle_websocket_message(socket: &mut WebSocket) -> Result<String, ()> {
    if let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            if let Ok(text) = msg.to_text() {
                if let Ok(ws_msg) = serde_json::from_str::<SocketMessageRecv>(text) {
                    if ws_msg.event.as_str() == "jwt" {
                        if let Some(jwt) = ws_msg.jwt {
                            return Ok(jwt);
                        }
                    } else {
                        println!("Unknown event: {}", ws_msg.event);
                    }
                }
            }
        }
    }

    Ok("".to_string())
}

pub async fn broadcast_trade(trade: Trade, sockets: Arc<Mutex<SocketList>>) {
    let socket_list = sockets.lock().await;
    for (_, socket_sender) in socket_list.iter() {
        if let Err(err) = socket_sender
            .send(SocketMessageSend::Trade(trade.clone()))
            .await
        {
            eprintln!("{}", err);
        }
    }
}

// pub async fn broadcast_order(order: Order, sockets: Arc<Mutex<SocketList>>) {
//     let senders: HashMap<_, _> = {
//         let socket_list = sockets.lock().await;
//         socket_list.clone()
//     };
//
//     for (_, socket_sender) in senders {
//         println!("sending a msg to: {:?}", socket_sender);
//         match socket_sender.send(SocketMessageSend::Order(order)).await {
//             Ok(_) => println!("Msg sent to the socket reader"),
//             Err(err) => println!("Error sending the msg to socket reader:\n{}", err),
//         }
//     }
// }
