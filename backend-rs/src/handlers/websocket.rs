use axum::extract::ws::{self, Message as WsMessage, WebSocket};
use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response as AxumResponse,
};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::domain::position::Trade;
use crate::types::SocketMessage;

pub type SocketList = Vec<mpsc::Sender<SocketMessage>>;

pub async fn ws_handler(
    State(sockets): State<Arc<Mutex<SocketList>>>,
    ws: WebSocketUpgrade,
) -> AxumResponse {
    let sockets_ = sockets.clone();
    ws.on_upgrade(|socket| handle_socket(socket, sockets))
}

pub async fn test(mut socket: WebSocket) {
    println!("some websocket connected");
    socket
        .send(ws::Message::text("wzzup. bejing!".to_string()))
        .await;
}

pub async fn handle_socket(mut socket: WebSocket, sockets: Arc<Mutex<SocketList>>) {
    println!("Some ws client connected");

    let (socket_tx, mut socket_rx) = mpsc::channel::<SocketMessage>(1000);

    {
        let mut socket_list = sockets.lock().await;
        socket_list.push(socket_tx);
        println!("socket_list len: {}", socket_list.len());
    }

    loop {
        if let Some(msg) = socket_rx.recv().await {
            match msg {
                SocketMessage::Trade(trade) => {
                    if let Ok(json) = serde_json::to_string(&trade) {
                        socket.send(ws::Message::text(json)).await;
                    }
                }
            }
        }
    }
}

pub async fn broadcast_trade(trade: Trade, sockets: Arc<Mutex<SocketList>>) {
    let socket_list = sockets.lock().await;
    for socket_sender in socket_list.iter() {
        socket_sender
            .send(SocketMessage::Trade(trade.clone()))
            .await;
    }
}

// pub async fn broadcast_order(order: Order, sockets: Arc<Mutex<SocketList>>) {
//     let senders: Vec<_> = {
//         let socket_list = sockets.lock().await;
//         socket_list.clone()
//     };
//
//     for socket_sender in senders {
//         println!("sending a msg to: {:?}", socket_sender);
//         match socket_sender.send().await {
//             Ok(_) => println!("Msg sent to the socket reader"),
//             Err(err) => println!("Error sending the msg to socket reader:\n{}", err),
//         }
//     }
// }
