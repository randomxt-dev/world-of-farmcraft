//! `among-server`: run the realmd + world server standalone.

use anyhow::Result;
use clap::Parser;

use among_server::{Accounts, ServerConfig};

#[derive(Parser)]
#[command(about = "A minimal WoW 1.12.1 realmd + world server for Among Azeroth")]
struct Args {
    /// The interface to listen on; 0.0.0.0 lets LAN players join.
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    #[arg(long, default_value_t = benilla_protocol::AUTH_PORT)]
    auth_port: u16,
    #[arg(long, default_value_t = benilla_protocol::WORLD_PORT)]
    world_port: u16,
    #[arg(long, default_value = "Among Azeroth")]
    realm_name: String,
    /// An account as NAME:PASSWORD; repeat for more players.
    #[arg(long = "account", value_name = "NAME:PASSWORD", required = true)]
    accounts: Vec<String>,
    /// Fewest players the host may start with (the rules never allow fewer than 3).
    #[arg(long, default_value_t = 4)]
    min_players: usize,
    /// Impostors asked for; the player count caps it.
    #[arg(long, default_value_t = 2)]
    impostors: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let accounts = Accounts::default();
    for entry in &args.accounts {
        let Some((name, pass)) = entry.split_once(':') else {
            anyhow::bail!("--account takes NAME:PASSWORD, got {entry:?}");
        };
        accounts.register(name, pass)?;
    }
    among_server::run(
        ServerConfig {
            bind: args.bind,
            auth_port: args.auth_port,
            world_port: args.world_port,
            realm_name: args.realm_name,
            settings: among_rules::Settings {
                min_players: args.min_players,
                impostors: args.impostors,
                ..among_rules::Settings::default()
            },
            ..ServerConfig::default()
        },
        accounts,
    )
    .await
}
