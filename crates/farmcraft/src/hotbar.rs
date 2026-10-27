//! The hotbar's client half: `hotbar.lua` in the stock UI's VM, which turns the main action bar
//! into the farmer's tool and seed picker driven by the mouse wheel and the number keys.
//!
//! `FARMCRAFT_SELFTEST=1` checks it in a live run: a few seconds in the world, it turns the wheel
//! down twice through the binding path, and logs the pick and the wheel bindings.
//! `FARMCRAFT_SELFTEST=ride` instead presses the horse button, and `FARMCRAFT_SELFTEST=map` opens
//! the world map, for screenshots of the mount and the farm's map icon.

use benilla_ui::script::UiScript;
use bevy::prelude::*;

use crate::lua_hooks;

pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        lua_hooks::register(app, "hotbar", include_str!("hotbar.lua").to_owned());
        lua_hooks::register(app, "markers", include_str!("markers.lua").to_owned());
        if std::env::var_os("FARMCRAFT_SELFTEST").is_some() {
            app.add_systems(PostUpdate, self_test);
        }
    }
}

fn self_test(
    script: Option<NonSend<UiScript>>,
    time: Res<Time>,
    hooked: Res<lua_hooks::HookedSession>,
    mut since: Local<Option<f32>>,
    mut done: Local<bool>,
) {
    let (Some(script), Some(_)) = (script, hooked.0) else {
        return;
    };
    if *done {
        return;
    }
    let now = time.elapsed_secs();
    let start = *since.get_or_insert(now);
    if now - start < 8.0 {
        return;
    }
    *done = true;
    let lua = match std::env::var("FARMCRAFT_SELFTEST").as_deref() {
        Ok("ride") => Some("UseAction(5) return 'ok'"),
        Ok("map") => Some("ToggleWorldMap() return 'ok'"),
        _ => None,
    };
    if let Some(lua) = lua {
        let done = script.eval::<String>(lua);
        info!("FARMCRAFT_SELFTEST: {done:?}");
        return;
    }
    let report = script.eval::<String>(
        r#"
        CameraZoomOut(1.0)
        CameraZoomOut(1.0)
        return "slot " .. FARMCRAFT_SLOT
            .. ", lit " .. tostring(ActionButton1:GetChecked()) .. "/" .. tostring(ActionButton2:GetChecked())
            .. ", has " .. tostring(HasAction(1)) .. "/" .. tostring(HasAction(2)) .. "/" .. tostring(HasAction(3))
            .. ", shift-wheel " .. tostring(GetBindingAction("SHIFT-MOUSEWHEELUP"))
            .. ", wheel " .. tostring(GetBindingAction("MOUSEWHEELUP"))
            .. ", tex " .. tostring(GetActionTexture(1)) .. "/" .. tostring(GetActionTexture(6))
            .. ", has12 " .. tostring(HasAction(12)) .. ", count6 " .. tostring(GetActionCount(6))
        "#,
    );
    match report {
        Ok(line) => info!("FARMCRAFT_SELFTEST: {line}"),
        Err(e) => error!("FARMCRAFT_SELFTEST: failed: {e}"),
    }
}
