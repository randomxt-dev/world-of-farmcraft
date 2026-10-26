//! A minimal WoW 1.12.1 (build 5875) server for World of Farmcraft: realmd (SRP6 logon, one
//! realm) and a world server that seats each farmer on Stonefield Farm and runs their farm. The
//! stock benilla client logs in to it unchanged, so movement, camera and the interface stay
//! regular WoW.

pub mod accounts;
pub mod game;
pub mod ground;
pub mod layout;
pub mod realmd;
pub mod saves;
pub mod srp;
pub mod world;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

pub use accounts::{Accounts, PASSWORD};
pub use layout::Layout;
pub use saves::Saves;

/// Where the server listens, where it keeps the save, and where the game data is.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind: String,
    /// `0` picks a free port.
    pub auth_port: u16,
    pub world_port: u16,
    pub realm_name: String,
    /// The save file; `None` keeps the farm in memory.
    pub save: Option<PathBuf>,
    /// The 1.12.1 `Data` folder the ground heights are read from; `None` lays the farm flat.
    pub data: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".into(),
            auth_port: benilla_protocol::AUTH_PORT,
            world_port: benilla_protocol::WORLD_PORT,
            realm_name: "World of Farmcraft".into(),
            save: None,
            data: None,
        }
    }
}

/// Both listeners, bound and not yet serving.
pub struct Server {
    auth: TcpListener,
    world: TcpListener,
    realm_name: String,
    accounts: Accounts,
    saves: Saves,
    layout: Arc<Layout>,
}

impl Server {
    /// Lay out the farm, open the save and bind both ports.
    pub async fn bind(config: &ServerConfig) -> Result<Self> {
        let centre = (-9915.0, 360.0);
        let ground = config.data.as_deref().and_then(|data| {
            match ground::Ground::open(data, centre.0, centre.1) {
                Ok(g) => Some(g),
                Err(e) => {
                    tracing::warn!("farm-server: no ground heights ({e:#}); the farm is flat");
                    None
                }
            }
        });
        let layout = Arc::new(Layout::stonefield(ground.as_ref()));
        let saves = Saves::open(config.save.clone(), layout.tiles.len(), layout.veins.len())?;
        let auth = TcpListener::bind((config.bind.as_str(), config.auth_port))
            .await
            .with_context(|| format!("binding realmd on {}:{}", config.bind, config.auth_port))?;
        let world = TcpListener::bind((config.bind.as_str(), config.world_port))
            .await
            .with_context(|| {
                format!(
                    "binding the world server on {}:{}",
                    config.bind, config.world_port
                )
            })?;
        Ok(Self {
            auth,
            world,
            realm_name: config.realm_name.clone(),
            accounts: Accounts::new(saves.clone()),
            saves,
            layout,
        })
    }

    /// realmd's address, the client's `WOW_HOST`.
    pub fn auth_addr(&self) -> SocketAddr {
        self.auth
            .local_addr()
            .expect("a bound listener has an address")
    }

    pub fn world_addr(&self) -> SocketAddr {
        self.world
            .local_addr()
            .expect("a bound listener has an address")
    }

    /// Serve until a listener fails.
    pub async fn serve(self) -> Result<()> {
        tracing::info!(
            "farm-server: realmd on {}, world on {}",
            self.auth_addr(),
            self.world_addr()
        );
        let realm = realmd::RealmConfig {
            name: self.realm_name,
            advertise: None,
            world_port: self.world.local_addr()?.port(),
        };
        tokio::try_join!(
            realmd::serve(self.auth, self.accounts.clone(), realm),
            world::serve(self.world, self.accounts, self.saves, self.layout),
        )?;
        Ok(())
    }
}
