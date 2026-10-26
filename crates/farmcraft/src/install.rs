//! Finding the player's own WoW 1.12.1 install before the client starts. benilla reads the game
//! from `$WOW_DATA` or a `Data` folder beside the exe; a downloaded launcher has neither, so the
//! first run asks for the install folder once and remembers it. Nothing is copied or written into
//! the install.

use std::path::{Path, PathBuf};

/// MPQs every English 1.12.1 `Data` folder holds; their presence is what we check.
const REQUIRED_MPQS: [&str; 4] = ["dbc.MPQ", "model.MPQ", "terrain.MPQ", "patch.MPQ"];

/// Where the remembered install lives: benilla's own state folder (`$BENILLA_HOME`, else
/// `benilla-config/` beside the exe), in our `farmcraft/` subfolder.
fn saved_path_file() -> Option<PathBuf> {
    let home = match std::env::var_os("BENILLA_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_exe()
            .ok()?
            .parent()?
            .join("benilla-config"),
    };
    Some(home.join("farmcraft").join("wow-path.txt"))
}

/// The `Data` folder in `chosen`, which may be the install folder or its `Data` folder itself,
/// when it holds the 1.12.1 MPQs.
pub fn data_folder(chosen: &Path) -> Option<PathBuf> {
    [chosen.join("Data"), chosen.to_path_buf()]
        .into_iter()
        .find(|dir| REQUIRED_MPQS.iter().all(|mpq| has_file(dir, mpq)))
}

/// Whether `dir` holds `name`, ignoring case as Windows does and a copied install may not.
fn has_file(dir: &Path, name: &str) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
    })
}

/// Point `$WOW_DATA` at the player's install when benilla would not find one: the remembered
/// folder, else one the player picks now. Call first thing in `main`, before any thread starts.
pub fn ensure_wow_data() {
    if benilla_formats::wow_data().is_some() {
        return;
    }
    let file = saved_path_file();
    let saved = file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| data_folder(Path::new(s.trim())));
    let data = saved.or_else(|| {
        let data = pick_install()?;
        if let Some(f) = &file {
            let remembered = f.parent().map(std::fs::create_dir_all).and_then(Result::ok);
            if remembered.is_some() {
                let _ = std::fs::write(f, data.to_string_lossy().as_bytes());
            }
        }
        Some(data)
    });
    if let Some(data) = data {
        std::env::set_var("WOW_DATA", data);
    }
}

/// Ask for the install folder until it holds the MPQs or the player cancels.
fn pick_install() -> Option<PathBuf> {
    loop {
        let chosen = rfd::FileDialog::new()
            .set_title("World of Farmcraft: choose your World of Warcraft 1.12.1 folder")
            .pick_folder()?;
        if let Some(data) = data_folder(&chosen) {
            return Some(data);
        }
        let retry = rfd::MessageDialog::new()
            .set_title("World of Farmcraft")
            .set_description(format!(
                "{} is not a WoW 1.12.1 install: its Data folder needs {}.",
                chosen.display(),
                REQUIRED_MPQS.join(", ")
            ))
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show();
        if retry != rfd::MessageDialogResult::Ok {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_install(test: &str, names: &[&str]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("farmcraft-install-{test}-{}", std::process::id()));
        let data = root.join("Data");
        std::fs::create_dir_all(&data).unwrap();
        for name in names {
            std::fs::write(data.join(name), b"").unwrap();
        }
        root
    }

    #[test]
    fn the_install_or_its_data_folder_is_accepted() {
        let root = fake_install("ok", &["DBC.MPQ", "model.MPQ", "terrain.mpq", "patch.MPQ"]);
        assert_eq!(data_folder(&root), Some(root.join("Data")));
        assert_eq!(data_folder(&root.join("Data")), Some(root.join("Data")));
    }

    #[test]
    fn a_folder_without_the_mpqs_is_refused() {
        let root = fake_install("missing", &["dbc.MPQ"]);
        assert_eq!(data_folder(&root), None);
    }
}
