//! Our ability names and descriptions in the stock tooltip and spellbook. The client takes a spell's
//! text from `Spell.dbc` alone, so once the in-game FrameXML is up on a VM this installs
//! `ability_text.lua` into it, the way an addon hooks the stock UI; every new VM (login,
//! `/reload`) gets it again.

use std::fmt::Write as _;

use among_rules::Settings;
use benilla_ui::script::UiScript;
use bevy::prelude::*;

use crate::abilities::{abilities, Ability};

/// Installs the hook on each in-game VM.
pub struct AbilityTextPlugin;

impl Plugin for AbilityTextPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AbilityTextChunk(chunk(&abilities(&Settings::default()))))
            .init_resource::<HookedSession>()
            // After `Update`, where the client's own UI systems feed and tick the VM.
            .add_systems(PostUpdate, install_hook);
    }
}

/// The Lua the hook runs: our table, then `ability_text.lua`.
#[derive(Resource)]
pub struct AbilityTextChunk(pub String);

/// The VM session the hook was last installed on ([`UiScript::session`]).
#[derive(Resource, Default)]
pub struct HookedSession(pub Option<u64>);

/// Install the hook once per VM, as soon as that VM has the stock `GameTooltip`: the boot VM the
/// client holds outside the world has no frames, and the world-entry load hands its VM over whole.
fn install_hook(
    script: Option<NonSend<UiScript>>,
    chunk: Res<AbilityTextChunk>,
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
    match script.run(&chunk.0) {
        Ok(()) => info!("among-azeroth: ability text hooked on VM session {session}"),
        Err(e) => error!("among-azeroth: ability text hook failed: {e}"),
    }
}

/// Whether `GameTooltip.xml` has loaded on this VM: a global lookup, no chunk compiled.
fn has_game_tooltip(script: &UiScript) -> bool {
    (script.lua().globals())
        .contains_key("GameTooltip")
        .unwrap_or(false)
}

/// The hook's Lua: `AmongAzeroth_Abilities`, keyed by each spell's borrowed name, then the hook.
pub fn chunk(table: &[Ability]) -> String {
    let mut lua = String::from("AmongAzeroth_Abilities = {\n");
    for a in table {
        let _ = write!(
            lua,
            "  [{}] = {{ name = {}, cast = {}, description = {}",
            quote(a.borrowed),
            quote(&a.name),
            quote(&a.cast),
            quote(&a.description),
        );
        if let Some(rank) = &a.rank {
            let _ = write!(lua, ", rank = {}", quote(rank));
        }
        if let Some(cooldown) = &a.cooldown {
            let _ = write!(lua, ", cooldown = {}", quote(cooldown));
        }
        lua.push_str(" },\n");
    }
    lua.push_str("}\n");
    lua.push_str(include_str!("ability_text.lua"));
    lua
}

/// A Lua string literal.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests;
