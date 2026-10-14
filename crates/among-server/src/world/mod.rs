//! The world server: the `SMSG_AUTH_CHALLENGE`/`CMSG_AUTH_SESSION` handshake, the character
//! roster, world entry, and the shared map every logged-in player stands on.

mod map;
pub mod packets;
pub mod player;
mod session;
mod socket;
pub mod update;
mod wire;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

use crate::accounts::Accounts;

pub use map::Map;
pub use socket::Packet;

/// Accept world connections forever; every session shares one [`Map`].
pub async fn serve(listener: TcpListener, accounts: Accounts) -> Result<()> {
    let map = Map::default();
    loop {
        let (stream, peer) = listener
            .accept()
            .await
            .context("accepting a world session")?;
        let accounts = accounts.clone();
        let map = map.clone();
        tokio::spawn(async move {
            match session::run(stream, accounts, map).await {
                Ok(()) => tracing::info!("world {peer}: closed"),
                Err(e) => tracing::info!("world {peer}: {e:#}"),
            }
        });
    }
}
