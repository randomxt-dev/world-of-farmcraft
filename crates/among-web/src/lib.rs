//! The browser entry: wenilla's `start`, with our plugins added after the game's as the native
//! `among-azeroth` launcher does. `web-build.sh` names the module `wenilla` so upstream's
//! unchanged `web/index.html` and `boot.js` load it.

use wasm_bindgen::prelude::*;

use benilla_app::BuildId;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    // On wasm32 the run hands the event loop to the browser and returns at once: no exit to report.
    benilla_app::run_with(
        BuildId {
            version: env!("CARGO_PKG_VERSION"),
            describe: env!("BENILLA_GIT_DESCRIBE"),
            sha: env!("BENILLA_GIT_SHA"),
            short: env!("BENILLA_GIT_SHORT"),
            date: env!("BENILLA_GIT_DATE"),
            profile: env!("BENILLA_PROFILE"),
            project_dir: env!("BENILLA_PROJECT_DIR"),
            ..Default::default()
        },
        |app| {
            app.add_plugins(among_azeroth::AmongAzerothPlugins);
        },
    );
}
