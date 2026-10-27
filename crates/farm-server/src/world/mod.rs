//! The world server: the `SMSG_AUTH_CHALLENGE`/`CMSG_AUTH_SESSION` handshake, the character
//! roster, world entry, and each farmer's game.

pub mod creatures;
pub mod items;
pub mod objects;
pub mod packets;
pub mod player;
pub mod quests;
mod session;
pub mod socket;
pub mod spells;
pub mod update;
mod wire;

use std::sync::Arc;

use anyhow::{Context, Result};
use benilla_protocol::messages::opcode;
use tokio::net::TcpListener;

use crate::accounts::Accounts;
use crate::layout::Layout;
use crate::saves::Saves;

pub use socket::Packet;

/// `SMSG_DESTROY_OBJECT`: the object leaves the receiver's world.
pub fn destroy(guid: u64) -> Packet {
    Packet::new(opcode::SMSG_DESTROY_OBJECT, guid.to_le_bytes().to_vec())
}

/// Accept world connections forever.
pub async fn serve(
    listener: TcpListener,
    accounts: Accounts,
    saves: Saves,
    layout: Arc<Layout>,
) -> Result<()> {
    loop {
        let (stream, peer) = listener
            .accept()
            .await
            .context("accepting a world session")?;
        let (accounts, saves, layout) = (accounts.clone(), saves.clone(), layout.clone());
        tokio::spawn(async move {
            match session::run(stream, accounts, saves, layout).await {
                Ok(()) => tracing::info!("world {peer}: closed"),
                Err(e) => tracing::info!("world {peer}: {e:#}"),
            }
        });
    }
}
