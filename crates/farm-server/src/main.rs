//! `farm-server`: run the realmd + world server standalone, for a client started by hand.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(about = "A minimal WoW 1.12.1 realmd + world server for World of Farmcraft")]
struct Args {
    /// The interface to listen on.
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    #[arg(long, default_value_t = benilla_protocol::AUTH_PORT)]
    auth_port: u16,
    #[arg(long, default_value_t = benilla_protocol::WORLD_PORT)]
    world_port: u16,
    /// The save file.
    #[arg(long, default_value = "benilla-config/farmcraft/save.ron")]
    save: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let config = farm_server::ServerConfig {
        bind: args.bind,
        auth_port: args.auth_port,
        world_port: args.world_port,
        save: Some(args.save),
        data: benilla_formats::wow_data(),
        ..Default::default()
    };
    tracing::info!(
        "farm-server: any account name logs in with password {}",
        farm_server::PASSWORD
    );
    farm_server::Server::bind(&config).await?.serve().await
}
