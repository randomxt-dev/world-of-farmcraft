//! `among-server`: run the realmd + world server standalone.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use among_server::cli::ServerArgs;
use among_server::layout_file;

#[derive(Parser)]
#[command(about = "A minimal WoW 1.12.1 realmd + world server for Among Azeroth")]
struct Args {
    /// The interface to listen on; 0.0.0.0 lets LAN players join.
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    #[command(flatten)]
    server: ServerArgs,
    /// Check a layout against the marks, print what it holds, and exit.
    #[arg(long, value_name = "FILE")]
    check_layout: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    if let Some(path) = &args.check_layout {
        let (layout, _) = layout_file::load(path, &args.server.marks)?;
        println!("{}", layout_file::summary(&layout));
        return Ok(());
    }
    let prepared = args.server.prepare(args.bind)?;
    if let Some(code) = &prepared.lobby_code {
        tracing::info!("among-server: lobby code {code}: any account name logs in with it");
    }
    among_server::run(prepared.config, prepared.accounts).await
}
