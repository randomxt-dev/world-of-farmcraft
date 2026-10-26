//! Among Azeroth's client side: Bevy plugins added on top of the stock benilla client. Movement,
//! camera and controls stay benilla's; these only change what the stock UI shows.

pub mod abilities;
pub mod ability_text;
#[cfg(not(target_arch = "wasm32"))]
pub mod install;
pub mod landmarks;
pub mod lua_hooks;
pub mod quest_watch;
pub mod sabotage_view;

use bevy::app::{PluginGroup, PluginGroupBuilder};

/// Every Among Azeroth client plugin, added by the launcher after benilla's own.
pub struct AmongAzerothPlugins;

impl PluginGroup for AmongAzerothPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(lua_hooks::LuaHooksPlugin)
            .add(ability_text::AbilityTextPlugin)
            .add(quest_watch::QuestWatchPlugin)
            .add(sabotage_view::SabotageViewPlugin)
            .add(landmarks::NoLandmarksPlugin)
    }
}
