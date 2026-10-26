//! The `among-azeroth` launcher: the stock benilla client through `benilla_app::run_with`, with our
//! plugins added after the game's.

use benilla_app::BuildId;

fn main() -> benilla_app::AppExit {
    among_azeroth::install::ensure_wow_data();
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
        app.add_plugins(among_azeroth::AmongAzerothPlugins);
    })
}
