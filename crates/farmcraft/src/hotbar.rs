//! The hotbar's client half: `hotbar.lua` in the stock UI's VM, which turns the main action bar
//! into a seed picker driven by the mouse wheel and the number keys.
//!
//! `FARMCRAFT_SELFTEST=1` checks it in a live run: a few seconds in the world, it turns the wheel
//! down twice through the binding path, and logs the pick and the wheel bindings.

use benilla_ui::script::UiScript;
use bevy::prelude::*;

use crate::lua_hooks;

pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        lua_hooks::register(app, "hotbar", include_str!("hotbar.lua").to_owned());
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
    let report = script.eval::<String>(
        r#"
        CameraZoomOut(1.0)
        CameraZoomOut(1.0)
        return "slot " .. FARMCRAFT_SLOT
            .. ", lit " .. tostring(ActionButton1:GetChecked()) .. "/" .. tostring(ActionButton2:GetChecked())
            .. ", has " .. tostring(HasAction(1)) .. "/" .. tostring(HasAction(2)) .. "/" .. tostring(HasAction(3))
            .. ", shift-wheel " .. tostring(GetBindingAction("SHIFT-MOUSEWHEELUP"))
            .. ", wheel " .. tostring(GetBindingAction("MOUSEWHEELUP"))
            .. ", tex " .. tostring(GetActionTexture(1)) .. "/" .. tostring(GetActionTexture(3))
            .. ", has9 " .. tostring(HasAction(9)) .. ", count1 " .. tostring(GetActionCount(1))
        "#,
    );
    match report {
        Ok(line) => info!("FARMCRAFT_SELFTEST: {line}"),
        Err(e) => error!("FARMCRAFT_SELFTEST: failed: {e}"),
    }
}
