//! A map's layout as a hand-written TOML file: the emergency button, the tasks and their steps,
//! the sabotage stations and the creatures, each placed at a surveyed mark (by name) or at a
//! literal `[x, y, z]`. See `layouts/northshire.example.toml`.

use std::collections::BTreeMap;
use std::time::Duration;

use among_rules::{
    CreatureDef, CreatureId, Layout, Position, SabotageDef, SabotageKind, StepAction, TaskDef,
    TaskKind, TaskStep,
};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::survey::Mark;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutFile {
    button: Spot,
    #[serde(default)]
    tasks: Vec<TaskEntry>,
    #[serde(default)]
    sabotages: Vec<SabotageEntry>,
    #[serde(default)]
    creatures: Vec<CreatureEntry>,
}

/// A mark's name, or a literal position.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Spot {
    Mark(String),
    At([f32; 3]),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskEntry {
    name: String,
    kind: String,
    steps: Vec<StepEntry>,
}

/// Exactly one of `cast` (seconds) or `slay` (creature ids, with `count` kills, default 1).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepEntry {
    at: Spot,
    cast: Option<f32>,
    slay: Option<Vec<String>>,
    count: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SabotageEntry {
    kind: String,
    stations: Vec<Spot>,
    /// Seconds until the impostors win; makes it critical.
    deadline: Option<f32>,
}

/// A beast at `at` ([`CreatureDef::beast`]), with any stat overridden.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatureEntry {
    id: String,
    name: String,
    at: Spot,
    health: Option<u32>,
    damage: Option<u32>,
    swing: Option<f32>,
    aggro: Option<f32>,
    leash: Option<f32>,
    speed: Option<f32>,
    respawn: Option<f32>,
}

/// Parse a layout file's text, placing every named spot at its mark.
pub fn parse(text: &str, marks: &BTreeMap<String, Mark>) -> Result<Layout> {
    let file: LayoutFile = toml::from_str(text).context("reading the layout")?;
    let missing = unsurveyed(&file, marks);
    if !missing.is_empty() {
        bail!(
            "{} not surveyed yet (walk there and type .mark <name>): {}",
            match missing.len() {
                1 => "1 mark".to_owned(),
                n => format!("{n} marks"),
            },
            missing.join(", ")
        );
    }
    let spot = |s: &Spot, what: &str| -> Result<Position> {
        match s {
            Spot::At(p) => Ok(*p),
            Spot::Mark(name) => (marks.get(name))
                .map(|m| m.at)
                .ok_or_else(|| anyhow!("{what}: no mark named {name:?} (survey it with .mark)")),
        }
    };

    let mut creature_ids: BTreeMap<&str, CreatureId> = BTreeMap::new();
    let mut creatures = Vec::new();
    for (i, c) in file.creatures.iter().enumerate() {
        let what = format!("creature {:?}", c.id);
        if creature_ids.insert(&c.id, CreatureId(i)).is_some() {
            bail!("{what}: the id is used twice");
        }
        let base = CreatureDef::beast(&c.name, spot(&c.at, &what)?);
        let secs = |s: Option<f32>, d: Duration| s.map_or(d, Duration::from_secs_f32);
        creatures.push(CreatureDef {
            health: c.health.unwrap_or(base.health),
            damage: c.damage.unwrap_or(base.damage),
            swing: secs(c.swing, base.swing),
            aggro: c.aggro.unwrap_or(base.aggro),
            leash: c.leash.unwrap_or(base.leash),
            speed: c.speed.unwrap_or(base.speed),
            respawn: secs(c.respawn, base.respawn),
            ..base
        });
    }

    let mut tasks = Vec::new();
    for t in &file.tasks {
        let kind = match t.kind.as_str() {
            "common" => TaskKind::Common,
            "long" => TaskKind::Long,
            "short" => TaskKind::Short,
            other => bail!(
                "task {:?}: kind {other:?} is not common, long or short",
                t.name
            ),
        };
        if t.steps.is_empty() {
            bail!("task {:?}: no steps", t.name);
        }
        let mut steps = Vec::new();
        for (n, s) in t.steps.iter().enumerate() {
            let what = format!("task {:?} step {}", t.name, n + 1);
            let at = spot(&s.at, &what)?;
            let action = match (&s.cast, &s.slay) {
                (Some(secs), None) if s.count.is_none() => {
                    StepAction::Cast(Duration::from_secs_f32(*secs))
                }
                (None, Some(ids)) => StepAction::Slay {
                    creatures: (ids.iter())
                        .map(|id| {
                            (creature_ids.get(id.as_str()).copied())
                                .ok_or_else(|| anyhow!("{what}: no creature with id {id:?}"))
                        })
                        .collect::<Result<_>>()?,
                    count: s.count.unwrap_or(1),
                },
                _ => bail!("{what}: give either cast = <seconds>, or slay = [ids] with count"),
            };
            steps.push(TaskStep { at, action });
        }
        tasks.push(TaskDef {
            name: t.name.clone(),
            kind,
            steps,
        });
    }

    let mut sabotages = Vec::new();
    for s in &file.sabotages {
        let kind = match s.kind.as_str() {
            "lights" => SabotageKind::Lights,
            "comms" => SabotageKind::Comms,
            "oxygen" => SabotageKind::Oxygen,
            "reactor" => SabotageKind::Reactor,
            other => bail!("sabotage {other:?} is not lights, comms, oxygen or reactor"),
        };
        let what = format!("sabotage {:?}", s.kind);
        if sabotages.iter().any(|d: &SabotageDef| d.kind == kind) {
            bail!("{what}: listed twice");
        }
        if s.stations.is_empty() {
            bail!("{what}: no stations");
        }
        sabotages.push(SabotageDef {
            kind,
            stations: (s.stations.iter())
                .map(|st| spot(st, &what))
                .collect::<Result<_>>()?,
            deadline: s.deadline.map(Duration::from_secs_f32),
        });
    }

    Ok(Layout {
        emergency_button: spot(&file.button, "button")?,
        tasks,
        sabotages,
        creatures,
    })
}

/// Every mark the file names that the survey lacks, sorted and once each: the to-do list.
fn unsurveyed(file: &LayoutFile, marks: &BTreeMap<String, Mark>) -> Vec<String> {
    let steps = file
        .tasks
        .iter()
        .flat_map(|t| t.steps.iter().map(|s| &s.at));
    let stations = file.sabotages.iter().flat_map(|s| s.stations.iter());
    let creatures = file.creatures.iter().map(|c| &c.at);
    let mut names: Vec<String> = std::iter::once(&file.button)
        .chain(steps)
        .chain(stations)
        .chain(creatures)
        .filter_map(|s| match s {
            Spot::Mark(name) if !marks.contains_key(name) => Some(name.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Load the layout at `path` against the marks at `marks_path`.
pub fn load(path: &std::path::Path, marks_path: &std::path::Path) -> Result<Layout> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let marks = crate::survey::read_marks(marks_path)?;
    parse(&text, &marks).with_context(|| format!("in {}", path.display()))
}

/// One line per part of the layout, for `--check-layout`.
pub fn summary(l: &Layout) -> String {
    let count = |k| l.tasks.iter().filter(|t| t.kind == k).count();
    let sabotages: Vec<String> = l
        .sabotages
        .iter()
        .map(|s| format!("{:?}", s.kind))
        .collect();
    format!(
        "button at {:?}\n{} tasks: {} common, {} long, {} short\n{} creatures\nsabotages: {}",
        l.emergency_button,
        l.tasks.len(),
        count(TaskKind::Common),
        count(TaskKind::Long),
        count(TaskKind::Short),
        l.creatures.len(),
        if sabotages.is_empty() {
            "none".into()
        } else {
            sabotages.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(x: f32) -> Mark {
        Mark {
            map: 0,
            at: [x, 0.0, 0.0],
            facing: 0.0,
        }
    }

    fn marks(names: &[&str]) -> BTreeMap<String, Mark> {
        (names.iter().enumerate())
            .map(|(i, n)| (n.to_string(), mark(i as f32 * 10.0)))
            .collect()
    }

    const SMALL: &str = r#"
        button = "hall"

        [[tasks]]
        name = "Pick Peacebloom"
        kind = "common"
        steps = [{ at = "herb", cast = 5.0 }]

        [[tasks]]
        name = "Thin the wolves"
        kind = "short"
        steps = [{ at = "den", slay = ["wolf-1", "wolf-2"], count = 2 }]

        [[sabotages]]
        kind = "oxygen"
        stations = ["pump", [1.0, 2.0, 3.0]]
        deadline = 30.0

        [[creatures]]
        id = "wolf-1"
        name = "Young Wolf"
        at = "den"

        [[creatures]]
        id = "wolf-2"
        name = "Young Wolf"
        at = "den"
        health = 60
        respawn = 45.0
    "#;

    #[test]
    fn a_layout_places_everything_at_its_marks() {
        let l = parse(SMALL, &marks(&["hall", "herb", "den", "pump"])).unwrap();
        assert_eq!(l.emergency_button, [0.0, 0.0, 0.0]);
        assert_eq!(l.tasks[0].steps[0].at, [10.0, 0.0, 0.0]);
        assert_eq!(
            l.tasks[0].steps[0].action,
            StepAction::Cast(Duration::from_secs(5))
        );
        assert_eq!(
            l.tasks[1].steps[0].action,
            StepAction::Slay {
                creatures: vec![CreatureId(0), CreatureId(1)],
                count: 2
            }
        );
        assert_eq!(l.sabotages[0].stations, [[30.0, 0.0, 0.0], [1.0, 2.0, 3.0]]);
        assert_eq!(l.sabotages[0].deadline, Some(Duration::from_secs(30)));
        assert_eq!(l.creatures[0].health, 40, "the beast default");
        assert_eq!(l.creatures[1].health, 60);
        assert_eq!(l.creatures[1].respawn, Duration::from_secs(45));
        assert_eq!(l.creatures[1].at, [20.0, 0.0, 0.0]);
    }

    #[test]
    fn mistakes_name_what_and_where() {
        let err = |text: &str| format!("{:#}", parse(text, &marks(&["hall", "herb"])).unwrap_err());
        // Every missing mark at once, the survey's to-do list.
        assert_eq!(
            err(SMALL),
            "2 marks not surveyed yet (walk there and type .mark <name>): den, pump"
        );
        let e = err(r#"button = "nowhere""#);
        assert!(e.starts_with("1 mark not surveyed yet"), "{e}");
        let e = err(r#"button = "hall"
            [[tasks]]
            name = "X"
            kind = "medium"
            steps = [{ at = "herb", cast = 1.0 }]"#);
        assert!(e.contains("kind \"medium\""), "{e}");
        let e = err(r#"button = "hall"
            [[tasks]]
            name = "X"
            kind = "short"
            steps = [{ at = "herb", cast = 1.0, slay = ["a"] }]"#);
        assert!(e.contains("step 1: give either cast"), "{e}");
        let e = err(r#"button = "hall"
            [[tasks]]
            name = "X"
            kind = "short"
            steps = [{ at = "herb", slay = ["ghost"] }]"#);
        assert!(e.contains("no creature with id \"ghost\""), "{e}");
        let e = err("button = \"hall\"\ncolour = \"red\"");
        assert!(e.contains("colour"), "unknown keys are refused: {e}");
    }

    /// The shipped Northshire example parses once every mark it names is surveyed.
    #[test]
    fn the_northshire_example_is_a_valid_layout() {
        let text = include_str!("../layouts/northshire.example.toml");
        let names = example_mark_names(text);
        assert!(names.len() > 10, "{names:?}");
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let l = parse(text, &marks(&refs)).unwrap();
        assert!(l.tasks.iter().any(|t| t.kind == TaskKind::Common));
        assert!(l.tasks.iter().any(|t| t.kind == TaskKind::Long));
        assert!(l.tasks.iter().filter(|t| t.kind == TaskKind::Short).count() >= 2);
        assert_eq!(l.sabotages.len(), 4);
        assert!(!l.creatures.is_empty());
    }

    /// Every quoted name the example places something at: its survey checklist.
    fn example_mark_names(text: &str) -> Vec<String> {
        let file: toml::Value = toml::from_str(text).unwrap();
        let mut names = Vec::new();
        let mut push = |v: &toml::Value| {
            if let Some(s) = v.as_str() {
                names.push(s.to_owned());
            }
        };
        push(&file["button"]);
        for t in file["tasks"].as_array().unwrap() {
            for s in t["steps"].as_array().unwrap() {
                push(&s["at"]);
            }
        }
        for s in file["sabotages"].as_array().unwrap() {
            s["stations"].as_array().unwrap().iter().for_each(&mut push);
        }
        for c in file["creatures"].as_array().unwrap() {
            push(&c["at"]);
        }
        names.sort();
        names.dedup();
        names
    }
}
