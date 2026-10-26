//! Our ability names and descriptions in the stock tooltip and spellbook. The client takes a spell's
//! text from `Spell.dbc` alone, so `ability_text.lua` rewrites it in the UI VM ([`crate::lua_hooks`]).

use std::fmt::Write as _;

use among_rules::Settings;
use bevy::prelude::*;

use crate::abilities::{abilities, Ability};
use crate::lua_hooks::{self, quote};

/// Registers the ability-text hook.
pub struct AbilityTextPlugin;

impl Plugin for AbilityTextPlugin {
    fn build(&self, app: &mut App) {
        lua_hooks::register(app, "ability text", chunk(&abilities(&Settings::default())));
    }
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

#[cfg(test)]
mod tests;
