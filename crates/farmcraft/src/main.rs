//! The `farmcraft` launcher: the farm server on a background thread, then the stock benilla
//! client pointed at it and logged in, so a double-click goes straight to the character screen.

mod hotbar;
mod icon;
mod install;
mod lua_hooks;

use std::path::PathBuf;

use benilla_app::BuildId;
use farm_server::{Server, ServerConfig};

/// The account the launcher logs in as; any name works with [`farm_server::PASSWORD`].
const ACCOUNT: &str = "FARMER";

/// Where the farm is saved: benilla's own state folder (`$BENILLA_HOME`, else `benilla-config/`
/// beside the exe), in our `farmcraft/` subfolder.
fn save_path() -> Option<PathBuf> {
    let home = match std::env::var_os("BENILLA_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_exe()
            .ok()?
            .parent()?
            .join("benilla-config"),
    };
    Some(home.join("farmcraft").join("save.ron"))
}

/// Bind the server, on the stock ports or any free ones, and serve it on its own thread; the
/// address the client logs on at.
fn start_server() -> anyhow::Result<String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut config = ServerConfig {
        save: save_path(),
        data: benilla_formats::wow_data(),
        ..Default::default()
    };
    let server = match runtime.block_on(Server::bind(&config)) {
        Ok(server) => server,
        Err(e) => {
            tracing::info!("farmcraft: stock ports taken ({e:#}); using free ones");
            config.auth_port = 0;
            config.world_port = 0;
            runtime.block_on(Server::bind(&config))?
        }
    };
    let port = server.auth_addr().port();
    std::thread::Builder::new()
        .name("farm-server".into())
        .spawn(move || {
            if let Err(e) = runtime.block_on(server.serve()) {
                eprintln!("farm-server stopped: {e:#}");
            }
        })?;
    Ok(format!("127.0.0.1:{port}"))
}

fn main() -> benilla_app::AppExit {
    install::ensure_wow_data();
    // The map's own melons in the Stonefield field are scenery nobody can pick: the farm's crops
    // grow there instead.
    let _ = benilla_world::terrain_stream::HIDDEN_DOODADS.set(vec!["elwynnmelon".into()]);
    match start_server() {
        Ok(host) => {
            // The client reads these at startup: our server, and the login typed for the player.
            std::env::set_var("WOW_HOST", host);
            if std::env::var_os("WOW_USER").is_none() {
                std::env::set_var("WOW_USER", ACCOUNT);
                std::env::set_var("WOW_PASS", farm_server::PASSWORD);
            }
            std::env::set_var("WOW_ALLOW_ACCOUNT", "1");
        }
        Err(e) => eprintln!("farmcraft: the farm server could not start: {e:#}"),
    }
    let build = BuildId {
        version: env!("CARGO_PKG_VERSION"),
        describe: env!("BENILLA_GIT_DESCRIBE"),
        sha: env!("BENILLA_GIT_SHA"),
        short: env!("BENILLA_GIT_SHORT"),
        date: env!("BENILLA_GIT_DATE"),
        profile: env!("BENILLA_PROFILE"),
        project_dir: env!("BENILLA_PROJECT_DIR"),
        ..Default::default()
    };
    benilla_app::run_with(build, |app| {
        app.add_plugins((hotbar::HotbarPlugin, icon::IconPlugin));
    })
}
