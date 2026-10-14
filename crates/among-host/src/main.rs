//! `among-host`: one command to host a lobby. It runs the game server for native clients and, on
//! one web port, the browser page, its game data and the WebSocket relay to that server, all
//! behind the lobby code, then prints the invite links.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;

use among_server::cli::ServerArgs;

#[derive(Parser)]
#[command(about = "Host an Among Azeroth lobby for native and browser players")]
struct Args {
    /// Where the browser page listens; every interface, so friends can open the invite link.
    #[arg(long, default_value = "0.0.0.0:8090")]
    web: String,
    /// The interface the game server listens on for native clients.
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,
    /// The 1.12.1 `Data` folder browser players are streamed from (default: `$WOW_DATA`, a `Data`
    /// folder beside the exe, the install the launcher remembered, else the repo's `WoW/Data`).
    #[arg(long)]
    data: Option<PathBuf>,
    /// The browser build (default: `www` beside the exe, else `web-build.ps1`'s output).
    #[arg(long)]
    www: Option<PathBuf>,
    /// Also open a Cloudflare quick tunnel (`cloudflared`, no account) to the page and print its
    /// https link for friends; it closes with the host.
    #[arg(long)]
    tunnel: bool,
    #[command(flatten)]
    server: ServerArgs,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let prepared = args.server.prepare(args.bind)?;
    let code = prepared
        .lobby_code
        .clone()
        .context("the invite link needs a lobby code: add --lobby-code or drop --account")?;
    let (auth_port, world_port) = (prepared.config.auth_port, prepared.config.world_port);

    let data = args.data.unwrap_or_else(default_data);
    let www = args.www.unwrap_or_else(default_www);

    let server = among_server::Server::bind(&prepared.config, prepared.accounts).await?;
    let chain = Arc::new(
        benilla_formats::Chain::open(&data)
            .with_context(|| format!("opening the game data at {}", data.display()))?,
    );
    if !www.join("index.html").exists() {
        tracing::warn!(
            "no browser build at {}: run crates/among-web/web-build.ps1 (native players can still join)",
            www.display()
        );
    }
    // The relay dials the game server on this machine, whatever interface it listens on.
    let loopback: Arc<str> = Arc::from("127.0.0.1");
    let web = wenilla_host::data::router(chain)
        .merge(wenilla_host::ws::router_map([
            (auth_port, Arc::clone(&loopback)),
            (world_port, loopback),
        ]))
        .merge(wenilla_host::static_site::router(&www));
    let web = among_host::gated(web, code.clone());
    let listener = tokio::net::TcpListener::bind(&args.web)
        .await
        .with_context(|| format!("binding the web page on {}", args.web))?;
    let web_port = listener.local_addr()?.port();

    print_invite(&code, web_port);
    // Kept for the host's lifetime: dropping the child closes the tunnel.
    let _tunnel = if args.tunnel {
        Some(open_tunnel(web_port, code.clone())?)
    } else {
        None
    };
    tokio::try_join!(server.serve(), async {
        axum::serve(listener, web)
            .await
            .context("serving the web page")
    })?;
    Ok(())
}

/// Start `cloudflared` on the page and print the friends' link once it announces its address;
/// its errors are printed too, since a blocked tunnel otherwise fails silently.
fn open_tunnel(web_port: u16, code: String) -> Result<tokio::process::Child> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let exe = among_host::cloudflared().context(
        "--tunnel needs cloudflared: install it with `winget install Cloudflare.cloudflared`",
    )?;
    let mut child = tokio::process::Command::new(exe)
        .args(["tunnel", "--no-autoupdate", "--url"])
        .arg(format!("http://127.0.0.1:{web_port}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("starting cloudflared")?;
    let stderr = child.stderr.take().context("cloudflared's log")?;
    println!("Opening a Cloudflare tunnel...");
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = among_host::tunnel_url(&line) {
                println!();
                println!("Friends: {url}/?lobby={code}");
                println!();
            } else if line.contains(" ERR ") || line.contains("failed to") {
                eprintln!("cloudflared: {line}");
            }
        }
    });
    Ok(child)
}

/// The exe's folder, where a release keeps `www/` and `benilla-config/`.
fn exe_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
}

fn default_data() -> PathBuf {
    benilla_formats::wow_data()
        .or_else(remembered_install)
        .unwrap_or_else(|| PathBuf::from("WoW/Data"))
}

/// The `Data` folder `among-azeroth`'s first run saved in `benilla-config/among/wow-path.txt`.
fn remembered_install() -> Option<PathBuf> {
    let file = exe_dir()?.join("benilla-config/among/wow-path.txt");
    let data = PathBuf::from(std::fs::read_to_string(file).ok()?.trim());
    data.is_dir().then_some(data)
}

fn default_www() -> PathBuf {
    exe_dir()
        .map(|dir| dir.join("www"))
        .filter(|www| www.join("index.html").exists())
        .unwrap_or_else(|| Path::new("target/among-web/dist").to_path_buf())
}

fn print_invite(code: &str, web_port: u16) {
    println!();
    println!("Among Azeroth lobby is up. Lobby code: {code}");
    println!("Players log in with any account name and the lobby code as the password.");
    println!();
    println!("Browser (Chrome or Edge). The world needs WebGPU, which needs https or localhost:");
    println!("  this PC:  http://localhost:{web_port}/?lobby={code}");
    println!("  friends need an https link, either of:");
    println!("    Cloudflare: run `cloudflared tunnel --url http://localhost:{web_port}` and send");
    println!(
        "                the https://....trycloudflare.com address it prints + /?lobby={code}"
    );
    match among_host::tailscale_name() {
        Some(name) => {
            println!("    Tailscale:  https://{name}/?lobby={code}");
            println!("                after `tailscale funnel --bg {web_port}`");
        }
        None => println!("    Tailscale:  `tailscale funnel --bg {web_port}`, then its https link"),
    }
    println!();
    match among_host::lan_ip() {
        Some(ip) => println!("Native among-azeroth: realmlist {ip} (TCP 3724 and 8085 open)."),
        None => {
            println!("Native among-azeroth: realmlist this PC's address (TCP 3724 and 8085 open).")
        }
    }
    println!();
}
