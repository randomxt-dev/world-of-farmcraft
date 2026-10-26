//! Lua run in the client's own UI VM, the way an addon extends the stock interface. A plugin
//! registers a chunk in [`LuaHooks`]; once the in-game FrameXML is up on a VM, every chunk runs on
//! it once, and every new VM (login, `/reload`) gets them again.

use benilla_ui::script::UiScript;
use bevy::prelude::*;

/// Runs the registered hooks; added by the first plugin that registers one.
pub struct LuaHooksPlugin;

impl Plugin for LuaHooksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LuaHooks>()
            .init_resource::<HookedSession>()
            // After `Update`, where the client's own UI systems feed and tick the VM.
            .add_systems(PostUpdate, install_hooks);
    }
}

/// Register `chunk` as the hook `name`, adding [`LuaHooksPlugin`] if it is not yet.
pub fn register(app: &mut App, name: &'static str, chunk: String) {
    if !app.is_plugin_added::<LuaHooksPlugin>() {
        app.add_plugins(LuaHooksPlugin);
    }
    (app.world_mut().resource_mut::<LuaHooks>().0).push(LuaHook { name, chunk });
}

/// One hook: its name for the log, and its Lua.
pub struct LuaHook {
    pub name: &'static str,
    pub chunk: String,
}

/// The hooks, run in registration order.
#[derive(Resource, Default)]
pub struct LuaHooks(pub Vec<LuaHook>);

/// The VM session the hooks were last installed on ([`UiScript::session`]).
#[derive(Resource, Default)]
pub struct HookedSession(pub Option<u64>);

/// Run every hook once per VM, as soon as that VM has the stock `GameTooltip`: the boot VM the
/// client holds outside the world has no frames, and the world-entry load hands its VM over whole.
fn install_hooks(
    script: Option<NonSend<UiScript>>,
    hooks: Res<LuaHooks>,
    mut hooked: ResMut<HookedSession>,
) {
    let Some(script) = script else {
        return;
    };
    let session = script.session();

    if hooked.0 == Some(session) || !has_game_tooltip(&script) {
        return;
    }
    // Marked either way: a chunk that failed once fails the same way every frame.
    hooked.0 = Some(session);
    for hook in &hooks.0 {
        match script.run(&hook.chunk) {
            Ok(()) => info!("farmcraft: {} hooked on VM session {session}", hook.name),
            Err(e) => error!("farmcraft: {} hook failed: {e}", hook.name),
        }
    }
}

/// Whether `GameTooltip.xml` has loaded on this VM: a global lookup, no chunk compiled.
fn has_game_tooltip(script: &UiScript) -> bool {
    (script.lua().globals())
        .contains_key("GameTooltip")
        .unwrap_or(false)
}
