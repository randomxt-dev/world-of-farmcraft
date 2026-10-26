//! A minimal WoW 1.12.1 (build 5875) server for Among Azeroth: realmd (SRP6 logon, one realm) and a
//! world server that seats each player on their lobby's map and relays their movement. The stock
//! benilla client logs in to it unchanged, so movement, camera and controls stay regular WoW.

pub mod accounts;
mod announce;
pub mod cli;
pub mod game;
pub mod layout_file;
pub mod lobbies;
pub mod look;
pub mod realmd;
pub mod srp;
pub mod survey;
pub mod world;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::TcpListener;

pub use accounts::Accounts;
pub use game::GameHost;
pub use lobbies::{Lobbies, Lobby};
pub use survey::Survey;

/// Where `.mark` saves the survey unless told otherwise: our folder inside the gitignored
/// `benilla-config/` (CLAUDE.md: our files live there), relative to the working directory.
pub const DEFAULT_MARKS: &str = "benilla-config/among/marks.toml";

/// Where the codes of player-made lobbies are kept across restarts, beside the marks.
pub const DEFAULT_LOBBIES: &str = "benilla-config/among/lobbies.txt";

/// How often empty player-made lobbies are looked for and closed.
const SWEEP: Duration = Duration::from_secs(30);

/// How long a player-made lobby may stand empty before it closes.
pub const IDLE_CLOSE: Duration = Duration::from_secs(10 * 60);

/// Where the server listens and what it advertises.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The interface both listeners bind, e.g. `0.0.0.0` for LAN play or `127.0.0.1`.
    pub bind: String,
    /// `0` picks a free port.
    pub auth_port: u16,
    pub world_port: u16,
    pub realm_name: String,
    /// The host the realm list sends clients to for the world server; `None` is the address each
    /// client reached realmd on.
    pub advertise: Option<String>,
    /// The match's rules and the map's task, button and station spots.
    pub settings: among_rules::Settings,
    pub layout: among_rules::Layout,
    /// How the layout's spots look in the world.
    pub look: look::Look,
    /// The survey file `.mark` writes.
    pub marks: PathBuf,
    /// How long the start is counted down in chat before the game begins.
    pub start_countdown: std::time::Duration,
    /// Live lobbies at most, the operator's own included.
    pub max_lobbies: usize,
    /// How long a player-made lobby may stand empty before it closes.
    pub idle_close: Duration,
    /// The file player-made lobbies' codes are kept in, so they outlive a restart; `None` keeps
    /// them in memory only.
    pub lobbies_file: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".into(),
            auth_port: benilla_protocol::AUTH_PORT,
            world_port: benilla_protocol::WORLD_PORT,
            realm_name: "Among Azeroth".into(),
            advertise: None,
            settings: among_rules::Settings::default(),
            layout: game::placeholder_layout(),
            look: game::placeholder_look(),
            marks: DEFAULT_MARKS.into(),
            start_countdown: game::START_COUNTDOWN,
            max_lobbies: 8,
            idle_close: IDLE_CLOSE,
            lobbies_file: None,
        }
    }
}

/// Both listeners, bound and not yet serving.
pub struct Server {
    auth: TcpListener,
    world: TcpListener,
    realm_name: String,
    advertise: Option<String>,
    lobbies: Lobbies,
    marks: PathBuf,
}

impl Server {
    /// Bind both ports and open the operator's lobby for `accounts`.
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
        let lobbies = Lobbies::new(
            lobbies::Template {
                settings: config.settings.clone(),
                layout: config.layout.clone(),
                look: config.look.clone(),
                start_countdown: config.start_countdown,
            },
            config.max_lobbies.max(1),
            config.idle_close,
        );
        lobbies.add_fixed(accounts);
        if let Some(file) = &config.lobbies_file {
            lobbies.persist(file.clone());
        }
        Ok(Self {
            auth,
            world,
            realm_name: config.realm_name.clone(),
            advertise: config.advertise.clone(),
            lobbies,
            marks: config.marks.clone(),
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

    /// The live lobbies, for a host that creates them and checks codes.
    pub fn lobbies(&self) -> Lobbies {
        self.lobbies.clone()
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
            advertise: self.advertise,
            world_port: self.world.local_addr()?.port(),
        };
        let survey = Survey::open(&self.marks)?;
        tracing::info!("among-server: survey marks in {}", survey.path().display());
        let sweeper = self.lobbies.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(SWEEP);
            loop {
                sweeper.sweep(every.tick().await);
            }
        });
        tokio::try_join!(
            realmd::serve(self.auth, self.lobbies.clone(), realm),
            world::serve(self.world, self.lobbies, survey),
        )?;
        Ok(())
    }
}

/// Bind and serve.
pub async fn run(config: ServerConfig, accounts: Accounts) -> Result<()> {
    Server::bind(&config, accounts).await?.serve().await
}
