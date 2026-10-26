//! No city landmarks: the minimap's rim arrows to Stormwind and Goldshire, and their world-map
//! icons, come from `AreaPOI.dbc`, so dropping benilla's catalog leaves the sabotage flag as the
//! only marker. The flag is the server's `SMSG_GOSSIP_POI`, which draws without the catalog.

use bevy::prelude::*;

use benilla_app::AreaPoiRes;

/// Removes the landmark catalog once benilla has loaded it at startup.
pub struct NoLandmarksPlugin;

impl Plugin for NoLandmarksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, |mut commands: Commands| {
            commands.remove_resource::<AreaPoiRes>();
        });
    }
}
