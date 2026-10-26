//! `GET /ws/{port}`: wenilla's WebSocket-to-TCP relay, plus the lobby hint. Behind the gate the
//! request carries its page's lobby code; the relay tells the server which local address its
//! TCP connection comes from, so realmd can put a browser logon in that lobby whatever the name.

use std::sync::Arc;

use among_server::Lobbies;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::LobbyCode;

#[derive(Clone)]
struct RelayState {
    lobbies: Lobbies,
    /// The game ports on this machine the relay may dial: realmd and the world server.
    ports: Arc<[u16]>,
}

/// The `/ws/{port}` router for the game server's `ports` on loopback; mount it behind the gate.
pub fn router(lobbies: Lobbies, ports: impl IntoIterator<Item = u16>) -> Router {
    Router::new()
        .route("/ws/{port}", get(upgrade))
        .with_state(RelayState {
            lobbies,
            ports: ports.into_iter().collect(),
        })
}

async fn upgrade(
    State(state): State<RelayState>,
    Path(port): Path<u16>,
    code: Option<Extension<LobbyCode>>,
    ws: WebSocketUpgrade,
) -> Response {
    let (true, Some(Extension(LobbyCode(code)))) = (state.ports.contains(&port), code) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = relay(socket, &state.lobbies, port, &code).await {
            tracing::warn!(error = %e, port, "ws relay ended");
        }
    })
}

/// Dial the game port, hint the lobby for that connection, then pump bytes both ways until both
/// sides close, each direction passing its end-of-stream to the other (as wenilla's relay does).
async fn relay(ws: WebSocket, lobbies: &Lobbies, port: u16, code: &str) -> anyhow::Result<()> {
    let tcp = TcpStream::connect(("127.0.0.1", port)).await?;
    tcp.set_nodelay(true)?;
    // The server sees this connection from our local address; nothing is sent before the hint.
    let local = tcp.local_addr()?;
    lobbies.hint(local, code);
    let (mut tcp_r, mut tcp_w) = tcp.into_split();
    let (mut ws_tx, mut ws_rx) = ws.split();

    let tcp_to_ws = async {
        let mut buf = [0u8; 65536];
        loop {
            match tcp_r.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    // A Close already queued by the client's own is only flushed, not re-sent.
                    if ws_tx.send(Message::Close(None)).await.is_err() {
                        let _ = ws_tx.flush().await;
                    }
                    break;
                }
                Ok(n) => {
                    if ws_tx
                        .send(Message::binary(buf[..n].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    };
    let ws_to_tcp = async {
        loop {
            match ws_rx.next().await {
                Some(Ok(Message::Binary(data))) => {
                    if tcp_w.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => {
                    if tcp_w.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
            }
        }
        let _ = tcp_w.shutdown().await;
    };
    tokio::join!(tcp_to_ws, ws_to_tcp);
    lobbies.forget_hint(local);
    Ok(())
}
