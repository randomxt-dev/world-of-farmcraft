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

use crate::look::{self, CreatureLook, Look, NodeKind, NodeLook};
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

/// One of: `cast` alone (seconds at `at`); `gather` (node spots) with `cast` per node and
/// `count` nodes; `slay` (creature ids) with `count` kills. `count` defaults to 1, and `at` (the
/// area a player is pointed to) to the first node or creature. `node` names what a cast or
/// gather step is worked at ([`look::NODE_NAMES`]); without it, a copper vein.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepEntry {
    at: Option<Spot>,
    cast: Option<f32>,
    gather: Option<Vec<Spot>>,
    slay: Option<Vec<String>>,
    count: Option<u32>,
    node: Option<String>,
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
    parse_with_look(text, marks).map(|(layout, _)| layout)
}

/// [`parse`], and how the layout looks in the world: each step's node and each creature's model.
pub fn parse_with_look(text: &str, marks: &BTreeMap<String, Mark>) -> Result<(Layout, Look)> {
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
    let mut step_looks = Vec::new();
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
        let mut looks = Vec::new();
        for (n, s) in t.steps.iter().enumerate() {
            let what = format!("task {:?} step {}", t.name, n + 1);
            let count = s.count.unwrap_or(1);
            let (action, first) = match (&s.cast, &s.gather, &s.slay) {
                (Some(secs), None, None) if s.count.is_none() => {
                    (StepAction::Cast(Duration::from_secs_f32(*secs)), None)
                }
                (Some(secs), Some(spots), None) => {
                    let nodes: Vec<Position> = (spots.iter())
                        .map(|n| spot(n, &what))
                        .collect::<Result<_>>()?;
                    if nodes.len() > 64 {
                        bail!("{what}: at most 64 nodes to gather from");
                    }
                    if count == 0 || count as usize > nodes.len() {
                        bail!("{what}: count {count} needs 1 to {} nodes", nodes.len());
                    }
                    let first = nodes[0];
                    let action = StepAction::Gather {
                        nodes,
                        cast: Duration::from_secs_f32(*secs),
                        count,
                    };
                    (action, Some(first))
                }
                (None, None, Some(ids)) if !ids.is_empty() && count > 0 => {
                    let creatures: Vec<CreatureId> = (ids.iter())
                        .map(|id| {
                            (creature_ids.get(id.as_str()).copied())
                                .ok_or_else(|| anyhow!("{what}: no creature with id {id:?}"))
                        })
                        .collect::<Result<_>>()?;
                    let first = creatures_at(&creatures, &file, &spot, &what)?;
                    (StepAction::Slay { creatures, count }, Some(first))
                }
                _ => bail!(
                    "{what}: give cast = <seconds>; or gather = [spots], cast and count; \
                     or slay = [ids] and count"
                ),
            };
            let node = match (&action, &s.node) {
                (StepAction::Slay { .. }, None) => None,
                (StepAction::Slay { .. }, Some(_)) => {
                    bail!("{what}: a slay step is fought, not worked at a node")
                }
                (_, None) => Some(NodeLook::mine(&t.name, look::COPPER_VEIN)),
                (_, Some(name)) => Some(look::named(name).ok_or_else(|| {
                    anyhow!(
                        "{what}: node {name:?} is not one of {}",
                        look::NODE_NAMES.join(", ")
                    )
                })?),
            };
            let at = match (&s.at, first) {
                (Some(at), _) => spot(at, &what)?,
                (None, Some(first)) => first,
                (None, None) => bail!("{what}: a cast step needs at = <spot>"),
            };
            // A lone herb or vein is picked, not worked: gone for everyone once a crewmate has it.
            let resource = node
                .as_ref()
                .is_some_and(|n| matches!(n.kind, NodeKind::Herb | NodeKind::Mine));
            let step = match action {
                StepAction::Cast(time) if resource && !time.is_zero() => {
                    TaskStep::pick(at, time.as_secs_f32())
                }
                action => TaskStep { at, action },
            };
            looks.push(node);
            steps.push(step);
        }
        tasks.push(TaskDef {
            name: t.name.clone(),
            kind,
            steps,
        });
        step_looks.push(looks);
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

    let look = Look {
        steps: step_looks,
        creatures: creatures.iter().map(|_| CreatureLook::wolf()).collect(),
    };
    let layout = Layout {
        emergency_button: spot(&file.button, "button")?,
        tasks,
        sabotages,
        creatures,
    };
    Ok((layout, look))
}

/// Where the first of a Slay step's creatures spawns: the area the step points to by default.
fn creatures_at(
    creatures: &[CreatureId],
    file: &LayoutFile,
    spot: &impl Fn(&Spot, &str) -> Result<Position>,
    what: &str,
) -> Result<Position> {
    spot(&file.creatures[creatures[0].0].at, what)
}

/// Every mark the file names that the survey lacks, sorted and once each: the to-do list.
fn unsurveyed(file: &LayoutFile, marks: &BTreeMap<String, Mark>) -> Vec<String> {
    let steps = (file.tasks.iter())
        .flat_map(|t| t.steps.iter())
        .flat_map(|s| s.at.iter().chain(s.gather.iter().flatten()));
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

/// Load the layout at `path` against the marks at `marks_path`, with its look.
pub fn load(path: &std::path::Path, marks_path: &std::path::Path) -> Result<(Layout, Look)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let marks = crate::survey::read_marks(marks_path)?;
    parse_with_look(&text, &marks).with_context(|| format!("in {}", path.display()))
}

/// One line per part of the layout, for `--check-layout`.
pub fn summary(l: &Layout) -> String {
    let count = |k| l.tasks.iter().filter(|t| t.kind == k).count();
    let sabotages: Vec<String> = l
        .sabotages
        .iter()
        .map(|s| format!("{:?}", s.kind))
        .collect();
    let mut out = format!(
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
    );
    for line in gathering(l) {
        out.push('\n');
        out.push_str(&line);
    }
    out
}

/// One line per task that gathers: its scarcest step's spots, which cap how many crewmates it is
/// dealt to (a pick takes a node from everyone), and how many hold it at the layout's full count.
fn gathering(l: &Layout) -> Vec<String> {
    (l.tasks.iter())
        .filter_map(|t| {
            let (spots, count) = (t.steps.iter())
                .filter_map(|s| match &s.action {
                    StepAction::Gather { nodes, count, .. } => Some((nodes.len().min(64), *count)),
                    _ => None,
                })
                .min_by_key(|&(spots, _)| spots)?;
            let full = spots / count.max(1) as usize;
            let common = if t.kind == TaskKind::Common {
                " (common: left out of games with more crewmates)"
            } else {
                ""
            };
            Some(format!(
                "gather {:?}: {spots} spots, dealt to at most {spots} crewmates{common}; \
                 {count} each for up to {full}, fewer each past that",
                t.name
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::NodeLook;
    use among_rules::TaskId;

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
        // A lone herb is picked: one node, gone for everyone once a crewmate has it.
        assert_eq!(
            l.tasks[0].steps[0].action,
            StepAction::Gather {
                nodes: vec![[10.0, 0.0, 0.0]],
                cast: Duration::from_secs(5),
                count: 1
            }
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
        assert!(e.contains("step 1: give cast = <seconds>"), "{e}");
        let e = err(r#"button = "hall"
            [[tasks]]
            name = "X"
            kind = "short"
            steps = [{ at = "herb", slay = ["ghost"] }]"#);
        assert!(e.contains("no creature with id \"ghost\""), "{e}");
        let e = err("button = \"hall\"\ncolour = \"red\"");
        assert!(e.contains("colour"), "unknown keys are refused: {e}");
    }

    #[test]
    fn a_gather_step_lists_its_nodes_and_points_at_the_first() {
        let text = r#"button = "hall"
            [[tasks]]
            name = "Pick Peacebloom"
            kind = "common"
            steps = [{ gather = ["herb", [5.0, 5.0, 5.0]], cast = 5.0, count = 2 }]"#;
        let l = parse(text, &marks(&["hall", "herb"])).unwrap();
        let step = &l.tasks[0].steps[0];
        assert_eq!(step.at, [10.0, 0.0, 0.0], "the first node");
        assert_eq!(
            step.action,
            StepAction::Gather {
                nodes: vec![[10.0, 0.0, 0.0], [5.0, 5.0, 5.0]],
                cast: Duration::from_secs(5),
                count: 2
            }
        );
        let too_many = text.replace("count = 2", "count = 3");
        let e = format!(
            "{:#}",
            parse(&too_many, &marks(&["hall", "herb"])).unwrap_err()
        );
        assert!(e.contains("count 3 needs 1 to 2 nodes"), "{e}");
        let no_cast = text.replace("cast = 5.0, ", "");
        let e = format!(
            "{:#}",
            parse(&no_cast, &marks(&["hall", "herb"])).unwrap_err()
        );
        assert!(e.contains("gather = [spots], cast and count"), "{e}");
    }

    #[test]
    fn a_step_names_the_node_it_is_worked_at() {
        let text = r#"button = "hall"
            [[tasks]]
            name = "Pick Peacebloom"
            kind = "common"
            steps = [{ gather = ["herb"], cast = 5.0, node = "peacebloom" }]
            [[tasks]]
            name = "Smelt"
            kind = "long"
            steps = [{ at = "herb", cast = 3.0, node = "forge" }, { at = "herb", cast = 1.0 }]"#;
        let (l, look) = parse_with_look(text, &marks(&["hall", "herb"])).unwrap();
        assert!(look.covers(&l));
        let node = |t, s| look.node(TaskId(t), s).unwrap();
        assert_eq!(
            *node(0, 0),
            NodeLook::herb("Peacebloom", crate::look::PEACEBLOOM)
        );
        assert_eq!(node(1, 0).kind, NodeKind::Station);
        // No node named: a copper vein to mine, as a plain look has.
        assert_eq!(node(1, 1).kind, NodeKind::Mine);
        let wrong = text.replace("\"forge\"", "\"furnace\"");
        let e = format!(
            "{:#}",
            parse_with_look(&wrong, &marks(&["hall", "herb"])).unwrap_err()
        );
        assert!(
            e.contains("node \"furnace\"") && e.contains("peacebloom"),
            "{e}"
        );
    }

    /// The shipped Northshire example parses once every mark it names is surveyed.
    #[test]
    fn the_summary_says_how_many_crewmates_each_gathering_serves() {
        let l = parse(SMALL, &marks(&["hall", "herb", "den", "pump"])).unwrap();
        let text = summary(&l);
        let gather: Vec<&str> = text.lines().filter(|l| l.starts_with("gather")).collect();
        assert_eq!(gather.len(), 1, "{text}");
        assert!(
            gather[0].contains("1 spots, dealt to at most 1 crewmates"),
            "{text}"
        );
    }

    #[test]
    fn the_northshire_example_is_a_valid_layout() {
        let text = include_str!("../layouts/northshire.example.toml");
        let file: LayoutFile = toml::from_str(text).unwrap();
        let checklist = unsurveyed(&file, &BTreeMap::new());
        assert!(checklist.len() > 30, "{checklist:?}");
        let refs: Vec<&str> = checklist.iter().map(String::as_str).collect();
        let (l, look) = parse_with_look(text, &marks(&refs)).unwrap();
        // Every worked step names its node: herbs look like herbs, not the plain copper vein.
        let plain = Look::plain(&l);
        for (t, task) in l.tasks.iter().enumerate() {
            for s in 0..task.steps.len() {
                let id = TaskId(t);
                if let Some(n) = look.node(id, s) {
                    assert_ne!(Some(n), plain.node(id, s), "{} step {}", task.name, s + 1);
                }
            }
        }
        assert!(l.tasks.iter().any(|t| t.kind == TaskKind::Common));
        assert!(l.tasks.iter().any(|t| t.kind == TaskKind::Long));
        assert!(l.tasks.iter().filter(|t| t.kind == TaskKind::Short).count() >= 2);
        assert!(l
            .tasks
            .iter()
            .any(|t| matches!(t.steps[0].action, StepAction::Gather { count: 5, .. })));
        assert_eq!(l.sabotages.len(), 4);
        assert!(!l.creatures.is_empty());
    }
}
