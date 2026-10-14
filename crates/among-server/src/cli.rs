//! The server's command line, shared by `among-server` and `among-host`; each binary adds its own
//! `--bind`, since a host listens on every interface by default and a bare server on loopback.

use std::path::PathBuf;

use anyhow::Result;

use crate::accounts::{self, Accounts};
use crate::{game, layout_file, ServerConfig, DEFAULT_MARKS};

#[derive(clap::Args, Debug)]
pub struct ServerArgs {
    #[arg(long, default_value_t = benilla_protocol::AUTH_PORT)]
    pub auth_port: u16,
    #[arg(long, default_value_t = benilla_protocol::WORLD_PORT)]
    pub world_port: u16,
    #[arg(long, default_value = "Among Azeroth")]
    pub realm_name: String,
    /// The lobby's shared password: any account name logs in with it. Made up when neither this
    /// nor `--account` is given.
    #[arg(long, value_name = "CODE")]
    pub lobby_code: Option<String>,
    /// A fixed account as NAME:PASSWORD; repeat for more. Without `--lobby-code` only these log in.
    #[arg(long = "account", value_name = "NAME:PASSWORD")]
    pub accounts: Vec<String>,
    /// The host name or IP native clients are sent to for the world server (default: the address
    /// they reached realmd on). Needed behind port forwarding, where that is a private IP.
    #[arg(long, value_name = "HOST")]
    pub advertise: Option<String>,
    /// Fewest players the host may start with (the rules never allow fewer than 3).
    #[arg(long, default_value_t = 4)]
    pub min_players: usize,
    /// Impostors asked for; the player count caps it.
    #[arg(long, default_value_t = 2)]
    pub impostors: usize,
    /// The map: a layout TOML whose spots name surveyed marks (default: a placeholder).
    #[arg(long, value_name = "FILE")]
    pub layout: Option<PathBuf>,
    /// The survey file `.mark` writes and layouts read their marks from.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_MARKS)]
    pub marks: PathBuf,
    /// Seconds the start is counted down in chat; 0 starts on the press.
    #[arg(long, value_name = "SECONDS", default_value_t = 5)]
    pub countdown: u64,
}

/// A configured server, ready to bind.
pub struct Prepared {
    pub config: ServerConfig,
    pub accounts: Accounts,
    /// The open lobby's code, `None` when only `--account`s log in.
    pub lobby_code: Option<String>,
}

impl ServerArgs {
    pub fn prepare(self, bind: String) -> Result<Prepared> {
        let (layout, look) = match &self.layout {
            Some(path) => layout_file::load(path, &self.marks)?,
            None => (game::placeholder_layout(), game::placeholder_look()),
        };
        let lobby_code = match self.lobby_code {
            Some(code) => Some(code.to_ascii_uppercase()),
            None if self.accounts.is_empty() => Some(accounts::new_lobby_code()),
            None => None,
        };
        let accounts = match &lobby_code {
            Some(code) => Accounts::open_lobby(code)?,
            None => Accounts::default(),
        };
        for entry in &self.accounts {
            let Some((name, pass)) = entry.split_once(':') else {
                anyhow::bail!("--account takes NAME:PASSWORD, got {entry:?}");
            };
            accounts.register(name, pass)?;
        }
        Ok(Prepared {
            config: ServerConfig {
                bind,
                auth_port: self.auth_port,
                world_port: self.world_port,
                realm_name: self.realm_name,
                advertise: self.advertise,
                settings: among_rules::Settings {
                    min_players: self.min_players,
                    impostors: self.impostors,
                    ..among_rules::Settings::default()
                },
                layout,
                look,
                marks: self.marks,
                start_countdown: std::time::Duration::from_secs(self.countdown),
            },
            accounts,
            lobby_code,
        })
    }
}
