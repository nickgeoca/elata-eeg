use futures_util::{stream::SplitStream, SinkExt, StreamExt};
use tokio::sync::broadcast;
use warp::ws::{Message, WebSocket};

/// Splits a WebSocket connection and spawns a dedicated sender task.
///
/// This function takes a WebSocket and a broadcast receiver. It splits the socket,
/// creates a new task to forward all messages from the broadcast channel to the
/// WebSocket client, and returns the receiving half of the WebSocket for the
/// caller to handle incoming messages.
///
/// # Arguments
///
/// * `ws` - The WebSocket connection.
/// * `data_rx` - A broadcast receiver for binary data to be sent to the client.
///
/// # Returns
///
/// The `Stream` half of the WebSocket, which can be used to receive messages.
pub fn split_and_spawn_sender(
    ws: WebSocket,
    mut data_rx: broadcast::Receiver<Vec<u8>>,
) -> SplitStream<WebSocket> {
    let (mut ws_sender, ws_receiver) = ws.split();

    // Spawn a task to handle sending messages to the client
    tokio::spawn(async move {
        loop {
            match data_rx.recv().await {
                Ok(data_packet) => {
                    if ws_sender
                        .send(Message::binary(data_packet))
                        .await
                        .is_err()
                    {
                        // Error sending, client has likely disconnected
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    println!("WebSocket sender lagged by {} messages", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    // The broadcast channel is closed, so we can stop
                    break;
                }
            }
        }
    });

    ws_receiver
}