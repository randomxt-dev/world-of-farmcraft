//! The world server: the `SMSG_AUTH_CHALLENGE`/`CMSG_AUTH_SESSION` handshake, the character
//! roster, world entry, and the map each lobby's players share.

pub mod chat;
pub mod combat;
pub mod creatures;
pub mod death;
pub mod guid;
pub mod items;
mod map;
pub mod meeting;
pub mod objects;
pub mod packets;
pub mod player;
pub mod quests;
mod session;
mod socket;
pub mod spells;
pub mod update;
mod wire;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

use crate::lobbies::Lobbies;
use crate::survey::Survey;

pub use map::{destroy, Map};
pub use socket::Packet;

/// Accept world connections forever; each session joins the lobby its account logged on to.
pub async fn serve(listener: TcpListener, lobbies: Lobbies, survey: Survey) -> Result<()> {
    loop {
        let (stream, peer) = listener
            .accept()
            .await
            .context("accepting a world session")?;
        let lobbies = lobbies.clone();
        let survey = survey.clone();
        tokio::spawn(async move {
            match session::run(stream, lobbies, survey).await {
                Ok(()) => tracing::info!("world {peer}: closed"),
                Err(e) => tracing::info!("world {peer}: {e:#}"),
            }
        });
    }
}
