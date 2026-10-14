//! The map survey: walk the stock client to a spot and type `.mark <name>`, and the server records
//! exactly where you stand. Layouts are written against these measured marks, never guessed
//! coordinates.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Where a surveyor stood: WoW map id, world position in yards, facing in radians.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub map: u32,
    pub at: [f32; 3],
    pub facing: f32,
}

/// The marks file: `[marks.<name>]` tables.
#[derive(Debug, Default, Serialize, Deserialize)]
struct MarksFile {
    #[serde(default)]
    marks: BTreeMap<String, Mark>,
}

/// Read a marks file; a missing file is an empty survey.
pub fn read_marks(path: &Path) -> Result<BTreeMap<String, Mark>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let file: MarksFile =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(file.marks)
}

fn write_marks(path: &Path, marks: &BTreeMap<String, Mark>) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let file = MarksFile {
        marks: marks.clone(),
    };
    let text = toml::to_string_pretty(&file).context("encoding the marks")?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// The marks so far and the file they are saved to after every change; cheap to clone.
#[derive(Clone)]
pub struct Survey {
    path: PathBuf,
    marks: Arc<Mutex<BTreeMap<String, Mark>>>,
}

const USAGE: &str = "Survey: .loc, .mark <name>, .unmark <name>, .marks";

impl Survey {
    /// The survey saved at `path`, with whatever marks it already holds.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let marks = read_marks(&path)?;
        Ok(Self {
            path,
            marks: Arc::new(Mutex::new(marks)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The reply lines to a chat line from someone standing at `here`, or `None` if the line is
    /// not a dot command.
    pub fn command(&self, text: &str, here: Mark) -> Option<Vec<String>> {
        let rest = text.strip_prefix('.')?;
        let mut words = rest.split_whitespace();
        let verb = words.next().unwrap_or("");
        let name = words.next();
        let extra = words.next().is_some();
        let mut marks = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
        let reply = match (verb, name, extra) {
            ("loc", None, false) => vec![format!("You are at {}", describe(&here))],
            ("mark", Some(name), false) if valid_name(name) => {
                let moved = marks.insert(name.to_owned(), here).is_some();
                let saved = self.save(&marks);
                let verb = if moved { "Moved" } else { "Marked" };
                vec![format!("{verb} {name} at {}{saved}", describe(&here))]
            }
            ("mark", Some(_), false) => {
                vec!["Mark names use letters, digits, - and _ only.".into()]
            }
            ("unmark", Some(name), false) => {
                if marks.remove(name).is_some() {
                    let saved = self.save(&marks);
                    vec![format!("Removed {name}{saved}")]
                } else {
                    vec![format!("No mark named {name}.")]
                }
            }
            ("marks", None, false) => list(&marks),
            _ => vec![USAGE.into()],
        };
        Some(reply)
    }

    /// "" when the marks reached the file, else why not, as a suffix for the reply.
    fn save(&self, marks: &BTreeMap<String, Mark>) -> String {
        match write_marks(&self.path, marks) {
            Ok(()) => String::new(),
            Err(e) => {
                tracing::warn!("survey: {e:#}");
                format!(" (NOT saved: {e:#})")
            }
        }
    }
}

/// Bare TOML keys, so a mark name is usable unquoted in the marks and layout files.
fn valid_name(name: &str) -> bool {
    name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn describe(m: &Mark) -> String {
    let [x, y, z] = m.at;
    format!(
        "map {} ({x:.2}, {y:.2}, {z:.2}) facing {:.2}",
        m.map, m.facing
    )
}

/// The mark names, a few per line so each chat line stays short.
fn list(marks: &BTreeMap<String, Mark>) -> Vec<String> {
    if marks.is_empty() {
        return vec!["No marks yet.".into()];
    }
    let names: Vec<&str> = marks.keys().map(String::as_str).collect();
    let mut lines = vec![format!("{} marks:", names.len())];
    lines.extend(names.chunks(8).map(|c| c.join(", ")));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A survey file in a fresh temp folder, unique to this test.
    fn temp_survey(test: &str) -> Survey {
        let dir = std::env::temp_dir().join(format!("among-survey-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Survey::open(dir.join("nested").join("marks.toml")).unwrap()
    }

    const HERE: Mark = Mark {
        map: 0,
        at: [-8949.95, -132.493, 83.5312],
        facing: 1.5,
    };

    #[test]
    fn plain_chat_is_not_a_command() {
        let s = temp_survey("plain");
        assert_eq!(s.command("hello there", HERE), None);
    }

    #[test]
    fn a_mark_is_saved_and_survives_a_restart() {
        let s = temp_survey("save");
        let reply = s.command(".mark herb-north", HERE).unwrap();
        assert_eq!(
            reply,
            ["Marked herb-north at map 0 (-8949.95, -132.49, 83.53) facing 1.50"]
        );
        let elsewhere = Mark {
            at: [1.0, 2.0, 3.0],
            ..HERE
        };
        let reply = s.command(".mark herb-north", elsewhere).unwrap();
        assert!(reply[0].starts_with("Moved herb-north"), "{reply:?}");
        s.command(".mark abbey_hall", HERE).unwrap();

        let again = read_marks(s.path()).unwrap();
        assert_eq!(again.len(), 2);
        assert_eq!(again["herb-north"], elsewhere);
        assert_eq!(again["abbey_hall"], HERE);
        let reopened = Survey::open(s.path()).unwrap();
        assert_eq!(
            reopened.command(".marks", HERE).unwrap(),
            ["2 marks:", "abbey_hall, herb-north"]
        );
    }

    #[test]
    fn unmark_loc_and_mistakes() {
        let s = temp_survey("misc");
        assert_eq!(
            s.command(".loc", HERE).unwrap(),
            ["You are at map 0 (-8949.95, -132.49, 83.53) facing 1.50"]
        );
        assert_eq!(s.command(".marks", HERE).unwrap(), ["No marks yet."]);
        s.command(".mark gate", HERE).unwrap();
        assert_eq!(s.command(".unmark gate", HERE).unwrap(), ["Removed gate"]);
        assert_eq!(
            s.command(".unmark gate", HERE).unwrap(),
            ["No mark named gate."]
        );
        assert!(read_marks(s.path()).unwrap().is_empty());
        assert_eq!(
            s.command(".mark has space", HERE).unwrap(),
            [USAGE],
            "two words after .mark"
        );
        assert_eq!(
            s.command(".mark wolf.den", HERE).unwrap(),
            ["Mark names use letters, digits, - and _ only."]
        );
        assert_eq!(s.command(".dance", HERE).unwrap(), [USAGE]);
    }

    #[test]
    fn a_missing_file_is_an_empty_survey() {
        let path = std::env::temp_dir().join("among-survey-does-not-exist.toml");
        assert!(read_marks(&path).unwrap().is_empty());
    }
}
