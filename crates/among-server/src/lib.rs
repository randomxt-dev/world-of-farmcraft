//! A minimal WoW 1.12.1 (build 5875) server for Among Azeroth: realmd (SRP6 logon, one realm) and a
//! world server that seats every player on one map and relays their movement. The stock benilla
//! client logs in to it unchanged, so movement, camera and controls stay regular WoW.

pub mod accounts;
pub mod realmd;
pub mod srp;
pub mod world;

use std::net::SocketAddr;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

pub use accounts::Accounts;

/// Where the server listens and what it advertises.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The interface both listeners bind, e.g. `0.0.0.0` for LAN play or `127.0.0.1`.
    pub bind: String,
    /// `0` picks a free port.
    pub auth_port: u16,
    pub world_port: u16,
    pub realm_name: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".into(),
            auth_port: benilla_protocol::AUTH_PORT,
            world_port: benilla_protocol::WORLD_PORT,
            realm_name: "Among Azeroth".into(),
        }
    }
}

/// Both listeners, bound and not yet serving.
pub struct Server {
    auth: TcpListener,
    world: TcpListener,
    realm_name: String,
    accounts: Accounts,
}

impl Server {
    pub async fn bind(config: &ServerConfig, accounts: Accounts) -> Result<Self> {
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
            accounts,
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
            "among-server: realmd on {}, world on {}",
            self.auth_addr(),
            self.world_addr()
        );
        let realm = realmd::RealmConfig {
            name: self.realm_name,
            world_port: self.world.local_addr()?.port(),
        };
        tokio::try_join!(
            realmd::serve(self.auth, self.accounts.clone(), realm),
            world::serve(self.world, self.accounts),
        )?;
        Ok(())
    }
}

/// Bind and serve.
pub async fn run(config: ServerConfig, accounts: Accounts) -> Result<()> {
    Server::bind(&config, accounts).await?.serve().await
}
