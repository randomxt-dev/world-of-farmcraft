//! The game's face on the desktop: the window wears the Golden Crest as its icon and is titled
//! "World of Farmcraft". The crest is our own art (`assets/golden-crest.png`), cropped square
//! round its centre and scaled to 256 pixels.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;

const CREST: &[u8] = include_bytes!("../assets/golden-crest.png");
const TITLE: &str = "World of Farmcraft";
const SIZE: u32 = 256;

pub struct IconPlugin;

impl Plugin for IconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, dress_window);
    }
}

/// The crest as RGBA pixels: the middle square of the wide logo, scaled down.
fn crest() -> Option<(Vec<u8>, u32)> {
    let img = image::load_from_memory_with_format(CREST, image::ImageFormat::Png).ok()?;
    let (w, h) = (img.width(), img.height());
    let side = w.min(h);
    let square = img.crop_imm((w - side) / 2, (h - side) / 2, side, side);
    let small = square.resize_exact(SIZE, SIZE, image::imageops::FilterType::Triangle);
    Some((small.to_rgba8().into_raw(), SIZE))
}

/// Once the window exists, give it the crest and the title.
fn dress_window(
    mut done: Local<bool>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
) {
    if *done {
        return;
    }
    let Ok((entity, mut window)) = windows.single_mut() else {
        return;
    };
    let dressed = WINIT_WINDOWS.with_borrow(|winit| {
        let Some(win) = winit.get_window(entity) else {
            return false;
        };
        if let Some((rgba, size)) = crest() {
            if let Ok(icon) = winit::window::Icon::from_rgba(rgba.clone(), size, size) {
                win.set_window_icon(Some(icon));
            }
            // The window icon is the title bar's; the taskbar and Alt+Tab take their own.
            #[cfg(windows)]
            if let Ok(icon) = winit::window::Icon::from_rgba(rgba, size, size) {
                use winit::platform::windows::WindowExtWindows;
                win.set_taskbar_icon(Some(icon));
            }
        }
        true
    });
    if dressed {
        window.title = TITLE.into();
        *done = true;
    }
}
