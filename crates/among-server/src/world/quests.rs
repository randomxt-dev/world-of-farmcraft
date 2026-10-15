//! The quest log as the match fills it: each task on a player's list is a quest whose objective
//! lines count its steps, and one more quest counts the crew's tasks done. The client builds its
//! log from the owner's `PLAYER_QUEST_LOG` fields alone, asking each new quest's template by
//! `CMSG_QUEST_QUERY` (benilla-app `ui_quest_log.rs:1-4`, `:483-534`).

use among_rules::{CreatureDef, PlayerTask, StepAction, TaskDef, TaskId};
use benilla_protocol::messages::opcode;

use super::guid;
use super::socket::Packet;
use super::update::{self, Fields};
use super::wire::Body;
use crate::look::{Look, NodeKind};

/// `PLAYER_QUEST_LOG_1_1` (vmangos `UpdateFields_1_12_1.h`): 20 slots of three fields each, the
/// quest id, the counters with the state byte, and the timer.
const PLAYER_QUEST_LOG_1_1: u16 = 198;
/// `MAX_QUEST_LOG_SIZE` (vmangos `QuestDef.h:34`).
pub const SLOTS: usize = 20;
/// The crew's quest, in every player's first slot.
pub const CREW_QUEST: u32 = 90_000;
/// Task `t`'s quest id is `TASK_QUESTS + t`.
const TASK_QUESTS: u32 = 90_001;
/// The objective quads a template carries (vmangos `QUEST_OBJECTIVES_COUNT`).
const OBJECTIVES: usize = 4;
/// A counter is 6 bits wide (vmangos `Player.h:1100-1106`), so no count shows above 63.
const COUNTER_MAX: u32 = 63;
/// `QUEST_STATE_COMPLETE` (vmangos `Player.h:447-451`), the state byte in bits 24-31.
const STATE_COMPLETE: u32 = 0x01;
/// The client's gameobject-objective bit (vmangos `Quest.cpp:512-516`).
const GO_OBJECTIVE: u32 = 0x8000_0000;
/// `QUEST_METHOD` 2, an ordinary quest (vmangos `QuestDef.h`).
const METHOD_NORMAL: u32 = 2;
/// Rows sort by level, then title (`ui_quest_log.rs` `order_groups`): the crew's quest one level
/// under the tasks heads the list. Both stay yellow for a level-1 character (`GetDifficultyColor`).
const CREW_LEVEL: u32 = 1;
const TASK_LEVEL: u32 = 2;

/// A task's quest id.
pub fn task_quest(task: TaskId) -> u32 {
    TASK_QUESTS + task.0 as u32
}

/// A task quest's id back to its task.
pub fn quest_task(quest_id: u32) -> Option<TaskId> {
    Some(TaskId(quest_id.checked_sub(TASK_QUESTS)? as usize))
}

/// A gameobject objective naming `entry`: a counted line with no creature behind it, so the
/// client asks no creature's name and prints the line's own text (`ui_quest_log.rs:178-239`).
fn go_objective(entry: u32) -> u32 {
    entry.wrapping_neg() | GO_OBJECTIVE
}

/// One counted objective line: the entry it names, the count it needs, and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub entry: u32,
    pub required: u32,
    pub text: String,
}

/// What a quest template says: title, level, the objectives and description texts, and up to
/// four counted lines.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestText {
    pub title: String,
    pub level: u32,
    pub objectives: String,
    pub details: String,
    pub lines: Vec<Line>,
}

/// What one step asks, as its phrase in the objectives text and its counted line.
/// How many nodes a holder gathers at a Gather step, by step index; 0 keeps the layout's count.
pub type Need<'a> = &'a dyn Fn(usize) -> u32;

/// The verb a node is worked with, and its past tense.
fn verbs(kind: Option<NodeKind>) -> (&'static str, &'static str) {
    match kind {
        Some(NodeKind::Herb) => ("pick", "picked"),
        Some(NodeKind::Mine) => ("mine", "mined"),
        Some(NodeKind::Station) => ("use", "used"),
        None => ("work", "worked"),
    }
}

fn step_text(
    task: TaskId,
    step: usize,
    action: &StepAction,
    look: &Look,
    creatures: &[CreatureDef],
    need: u32,
) -> (String, Line) {
    let node = look.node(task, step);
    let name = node.map(|n| n.name.as_str());
    let (verb, done) = verbs(node.map(|n| n.kind));
    let (phrase, text, required) = match (action, name) {
        // One node to pick reads as the one thing: "mine the Copper Vein".
        (StepAction::Gather { count, nodes, .. }, Some(n)) => {
            let need = if need == 0 { *count } else { need };
            let phrase = if need == 1 && nodes.len() == 1 {
                format!("{verb} the {n}")
            } else {
                format!("{verb} {n} ({need})")
            };
            (phrase, format!("{n} {done}"), need)
        }
        (StepAction::Gather { count, .. }, None) => {
            let need = if need == 0 { *count } else { need };
            (format!("gather ({need})"), "Gathered".into(), need)
        }
        (
            StepAction::Slay {
                creatures: ids,
                count,
            },
            _,
        ) => {
            let n = (ids.first())
                .and_then(|id| creatures.get(id.0))
                .map_or("creatures", |c| c.name.as_str());
            (format!("slay {n} ({count})"), format!("{n} slain"), *count)
        }
        (StepAction::Cast(_), Some(n)) => (format!("{verb} the {n}"), format!("{n} {done}"), 1),
        (StepAction::Cast(_), None) => (
            format!("finish step {}", step + 1),
            format!("Step {} done", step + 1),
            1,
        ),
    };
    let line = Line {
        entry: go_objective(guid::node_entry(task, step)),
        required: required.min(COUNTER_MAX),
        text,
    };
    (phrase, line)
}

/// Task `task`'s quest: one line per step, or one "Steps done" line past four steps; a Gather
/// step counts the holder's `need`. An impostor's copy is marked fake (`cover`), as Among Us
/// heads their list "Fake Tasks": it never progresses, and only they see their log.
pub fn task_text(
    task: TaskId,
    def: &TaskDef,
    look: &Look,
    creatures: &[CreatureDef],
    need: Need,
    cover: bool,
) -> QuestText {
    let (phrases, mut lines): (Vec<String>, Vec<Line>) = (def.steps.iter().enumerate())
        .map(|(i, s)| step_text(task, i, &s.action, look, creatures, need(i)))
        .unzip();
    if lines.len() > OBJECTIVES {
        lines = vec![Line {
            entry: go_objective(guid::node_entry(task, 0)),
            required: (def.steps.len() as u32).min(COUNTER_MAX),
            text: "Steps done".into(),
        }];
    }
    let mut objectives = phrases.join(", then ");
    if let Some(first) = objectives.get(..1) {
        objectives = first.to_uppercase() + &objectives[1..];
    }
    objectives.push('.');
    QuestText {
        title: if cover {
            format!("{} (fake)", def.name)
        } else {
            def.name.clone()
        },
        level: TASK_LEVEL,
        objectives,
        details: if cover {
            "Your cover as an impostor: fake it where the crew can see you. It never finishes \
             and does not fill the crew's bar."
        } else {
            "One of your tasks. Every task a crewmate finishes fills the crew's bar, and a full \
             bar wins the game."
        }
        .into(),
        lines,
    }
}

/// The crew's quest, counting `total` tasks; a count past 63 shows as 63.
pub fn crew_text(total: usize) -> QuestText {
    QuestText {
        title: "Crew Tasks".into(),
        level: CREW_LEVEL,
        objectives: "Finish every task on the crew's lists.".into(),
        details: "The whole crew's tasks, counted together. When every one is done, the crew \
                  wins."
            .into(),
        lines: vec![Line {
            entry: GO_OBJECTIVE,
            required: (total as u32).min(COUNTER_MAX),
            text: "Crew tasks done".into(),
        }],
    }
}

/// `SMSG_QUEST_QUERY_RESPONSE` for `text` under the zone header `zone`, in the layout
/// benilla-protocol's `read_quest_query_response` reads (vmangos `Quest.cpp:393-528`).
pub fn template(quest_id: u32, zone: u32, text: &QuestText) -> Packet {
    let mut b = Body::default();
    b.u32(quest_id)
        .u32(METHOD_NORMAL)
        .u32(text.level)
        .u32(zone) // zoneOrSort: a positive id is an `AreaTable` zone header
        .u32(0) // type: no tag
        .u32(0) // repObjectiveFaction
        .u32(0) // repObjectiveValue
        .u32(0) // requiredOppositeRepFaction
        .u32(0) // requiredOppositeRepValue
        .u32(0) // nextQuestInChain
        .u32(0) // money
        .u32(0) // money at max level
        .u32(0) // reward spell
        .u32(0) // source item
        .u32(0); // flags: not shareable
    for _ in 0..4 + 6 {
        b.u32(0).u32(0); // rewards, then choices: (item, count)
    }
    b.u32(0).f32(0.0).f32(0.0).u32(0); // no point of interest
    b.cstring(&text.title)
        .cstring(&text.objectives)
        .cstring(&text.details)
        .cstring(""); // end text
    for i in 0..OBJECTIVES {
        let (entry, required) = text.lines.get(i).map_or((0, 0), |l| (l.entry, l.required));
        b.u32(entry).u32(required).u32(0).u32(0);
    }
    for i in 0..OBJECTIVES {
        b.cstring(text.lines.get(i).map_or("", |l| l.text.as_str()));
    }
    Packet::new(opcode::SMSG_QUEST_QUERY_RESPONSE, b.done())
}

/// Four 6-bit counters at bits `6i` and the state byte at bits 24-31 (vmangos `Player.h:1100`).
fn count_state(counters: &[u32], complete: bool) -> u32 {
    let word =
        (counters.iter().enumerate()).fold(0, |w, (i, &c)| w | c.min(COUNTER_MAX) << (6 * i));
    let state = if complete { STATE_COMPLETE } else { 0 };
    word | state << 24
}

/// A task's counters word: each finished step's line full, the current step's at its picks or
/// kills, the rest empty; complete once every step is done.
pub fn task_counters(def: &TaskDef, t: &PlayerTask, need: Need) -> u32 {
    let steps = def.steps.len();
    let done = t.step >= steps;
    if steps > OBJECTIVES {
        return count_state(&[t.step as u32], done);
    }
    let counters: Vec<u32> = (def.steps.iter().enumerate())
        .map(|(i, s)| {
            let required = match &s.action {
                StepAction::Cast(_) => 1,
                StepAction::Gather { count, .. } => match need(i) {
                    0 => *count,
                    n => n,
                },
                StepAction::Slay { count, .. } => *count,
            };
            match (i.cmp(&t.step), &s.action) {
                (std::cmp::Ordering::Less, _) => required,
                (std::cmp::Ordering::Equal, StepAction::Gather { .. }) => t.picked.count_ones(),
                (std::cmp::Ordering::Equal, StepAction::Slay { .. }) => t.kills,
                _ => 0,
            }
        })
        .collect();
    count_state(&counters, done)
}

/// The crew's counters word: `done` of `total`, complete when all are done.
pub fn crew_counters(done: usize, total: usize) -> u32 {
    count_state(&[done as u32], total > 0 && done >= total)
}

/// A whole quest log, per slot `(quest id, counters word)`; `(0, 0)` is an empty slot.
pub type Log = [(u32, u32); SLOTS];

/// A player's log: empty with no tasks, else the crew's quest then each task in list order, as
/// many as the slots hold.
pub fn player_log(
    tasks: &[PlayerTask],
    defs: &[TaskDef],
    crew: (usize, usize),
    need: &dyn Fn(TaskId, usize) -> u32,
) -> Log {
    let mut log = [(0, 0); SLOTS];
    if tasks.is_empty() {
        return log;
    }
    log[0] = (CREW_QUEST, crew_counters(crew.0, crew.1));
    for (slot, t) in log[1..].iter_mut().zip(tasks) {
        if let Some(def) = defs.get(t.task.0) {
            *slot = (
                task_quest(t.task),
                task_counters(def, t, &|s| need(t.task, s)),
            );
        }
    }
    log
}

/// What a client was last told of its log: the slots, and the crew total its crew template counts.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Told {
    pub log: Log,
    pub crew_total: Option<usize>,
    /// Each task quest's objectives as last sent, so a share that changed is sent again.
    pub texts: std::collections::HashMap<u32, String>,
}

/// The owner's values update for every slot that differs between `was` and `now`: all three of
/// its fields, the timer always 0. `None` when nothing changed.
pub fn log_update(guid: u64, was: &Log, now: &Log) -> Option<Packet> {
    let mut f = Fields::default();
    for (i, (&(id, counters), _)) in (now.iter().zip(was))
        .enumerate()
        .filter(|(_, (n, w))| n != w)
    {
        let base = PLAYER_QUEST_LOG_1_1 + 3 * i as u16;
        f.u32(base, id).u32(base + 1, counters).u32(base + 2, 0);
    }
    (now != was).then(|| Packet::new(opcode::SMSG_UPDATE_OBJECT, update::values(guid, &f)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use among_rules::{CreatureId, Layout, Position, TaskKind, TaskStep};
    use benilla_protocol::messages::{
        parse_server_with_tail, quest_slot_state, Object, QuestTemplate, ServerPacket,
    };
    use std::time::Duration;

    use crate::look::NodeLook;

    fn parse(p: &Packet) -> ServerPacket {
        let (packet, tail) =
            parse_server_with_tail(p.opcode, &p.body).expect("the client parses it");
        assert_eq!(tail, 0, "{:#06x} left bytes unread", p.opcode);
        packet
    }

    fn parse_template(p: &Packet) -> QuestTemplate {
        let ServerPacket::QuestQueryResponse(t) = parse(p) else {
            panic!("not a quest template");
        };
        *t
    }

    const AT: Position = [0.0, 0.0, 0.0];

    /// Gather 3 Peacebloom, use the Campfire, slay 2 wolves.
    fn three_steps() -> (TaskDef, Look, Vec<CreatureDef>) {
        let def = TaskDef {
            name: "Feed the Crew".into(),
            kind: TaskKind::Long,
            steps: vec![
                TaskStep {
                    at: AT,
                    action: StepAction::Gather {
                        nodes: vec![AT; 5],
                        cast: Duration::from_secs(5),
                        count: 3,
                    },
                },
                TaskStep::cast(AT, 3.0),
                TaskStep {
                    at: AT,
                    action: StepAction::Slay {
                        creatures: vec![CreatureId(0), CreatureId(1)],
                        count: 2,
                    },
                },
            ],
        };
        let look = Look {
            steps: vec![vec![
                Some(NodeLook::herb("Peacebloom", 269)),
                Some(NodeLook::station("Campfire", 192, 2540)),
                None,
            ]],
            creatures: Vec::new(),
        };
        let wolf = among_rules::CreatureDef::beast("Young Wolf", AT);
        (def, look, vec![wolf.clone(), wolf])
    }

    #[test]
    fn a_task_template_reads_back_with_a_counted_line_per_step() {
        let (def, look, creatures) = three_steps();
        let text = task_text(TaskId(0), &def, &look, &creatures, &|_| 0, false);
        let t = parse_template(&template(task_quest(TaskId(0)), 12, &text));
        assert_eq!(t.quest_id, 90_001);
        assert_eq!(t.title, "Feed the Crew");
        assert_eq!(t.zone_or_sort, 12);
        assert_eq!(t.level, TASK_LEVEL);
        assert_eq!(
            t.objectives_text,
            "Pick Peacebloom (3), then use the Campfire, then slay Young Wolf (2)."
        );
        assert_eq!(t.flags, 0, "not shareable");
        let lines: Vec<(u32, &str)> = (t.objectives.iter())
            .map(|o| (o.required_count, o.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (3, "Peacebloom picked"),
                (1, "Campfire used"),
                (2, "Young Wolf slain"),
                (0, "")
            ]
        );
        // A counted line needs a nonzero entry; the gameobject bit keeps the client from asking
        // a creature's name, and the empty fourth quad shows no line.
        for o in &t.objectives[..3] {
            assert_ne!(o.creature_or_go & GO_OBJECTIVE, 0);
            assert_eq!((o.item_id, o.item_count), (0, 0));
        }
        assert_eq!(t.objectives[3].creature_or_go, 0);
    }

    #[test]
    fn a_long_task_counts_its_steps_on_one_line() {
        let def = TaskDef {
            name: "Long Haul".into(),
            kind: TaskKind::Long,
            steps: vec![TaskStep::cast(AT, 1.0); 6],
        };
        let text = task_text(
            TaskId(2),
            &def,
            &Look::plain(&Layout::default()),
            &[],
            &|_| 0,
            false,
        );
        let t = parse_template(&template(task_quest(TaskId(2)), 12, &text));
        assert_eq!(t.objectives[0].text, "Steps done");
        assert_eq!(t.objectives[0].required_count, 6);
        assert_eq!(t.objectives[1].creature_or_go, 0);
        let t3 = PlayerTask {
            task: TaskId(2),
            step: 3,
            kills: 0,
            picked: 0,
        };
        assert_eq!(task_counters(&def, &t3, &|_| 0), 3);
    }

    #[test]
    fn the_crew_template_counts_the_total_capped_at_63() {
        let t = parse_template(&template(CREW_QUEST, 12, &crew_text(16)));
        assert_eq!((t.quest_id, t.title.as_str()), (CREW_QUEST, "Crew Tasks"));
        assert_eq!(t.objectives[0].required_count, 16);
        assert_eq!(t.objectives[0].text, "Crew tasks done");
        assert_ne!(t.objectives[0].creature_or_go, 0);
        let big = parse_template(&template(CREW_QUEST, 12, &crew_text(80)));
        assert_eq!(big.objectives[0].required_count, 63);
    }

    /// The owner's log fields as the client reads them back from a values update.
    fn slots_of(p: &Packet) -> Vec<(u8, benilla_protocol::messages::QuestLogSlot)> {
        let ServerPacket::UpdateObject { objects } = parse(p) else {
            panic!("not an update");
        };
        let [Object::Values { guid: 7, mask, .. }] = objects.as_slice() else {
            panic!("one values block for the owner, got {}", objects.len());
        };
        (0..SLOTS as u8)
            .filter_map(|s| mask.player_quest_log(s).map(|q| (s, q)))
            .collect()
    }

    #[test]
    fn a_log_reads_back_its_quests_counters_and_completion() {
        let (def, _, _) = three_steps();
        let defs = vec![def];
        let fresh = PlayerTask {
            task: TaskId(0),
            step: 0,
            kills: 0,
            picked: 0b10010,
        };
        let empty = [(0, 0); SLOTS];
        let now = player_log(&[fresh], &defs, (1, 4), &|_, _| 0);
        let slots = slots_of(&log_update(7, &empty, &now).unwrap());
        assert_eq!(slots.len(), 2, "the crew's quest and the task: {slots:?}");
        let (crew, task) = (slots[0].1, slots[1].1);
        assert_eq!((slots[0].0, crew.quest_id), (0, CREW_QUEST));
        assert_eq!((crew.counters[0], crew.state), (1, 0));
        assert_eq!((slots[1].0, task.quest_id), (1, 90_001));
        assert_eq!(task.counters, [2, 0, 0, 0], "two herbs picked");
        assert_eq!((task.state, task.timer), (0, 0));

        // On to the wolves: the herbs and the fire are full, one kill counted.
        let slaying = PlayerTask {
            step: 2,
            kills: 1,
            picked: 0,
            ..fresh
        };
        let later = player_log(&[slaying], &defs, (1, 4), &|_, _| 0);
        let slots = slots_of(&log_update(7, &now, &later).unwrap());
        assert_eq!(slots.len(), 1, "only the changed slot: {slots:?}");
        assert_eq!(slots[0].1.counters, [3, 1, 1, 0]);

        let done = PlayerTask { step: 3, ..slaying };
        let finished = player_log(&[done], &defs, (4, 4), &|_, _| 0);
        let slots = slots_of(&log_update(7, &later, &finished).unwrap());
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].1.state, quest_slot_state::COMPLETE, "the crew's");
        assert_eq!(slots[1].1.counters, [3, 1, 2, 0]);
        assert_eq!(slots[1].1.state, quest_slot_state::COMPLETE);

        // No tasks clears every slot that held a quest.
        let cleared = player_log(&[], &defs, (0, 0), &|_, _| 0);
        assert_eq!(cleared, empty);
        let slots = slots_of(&log_update(7, &finished, &cleared).unwrap());
        assert!(slots.iter().all(|(_, q)| q.quest_id == 0 && q.state == 0));
        assert_eq!(slots.len(), 2);
        assert!(log_update(7, &cleared, &cleared).is_none());
    }

    #[test]
    fn task_quest_ids_round_trip_and_the_crew_is_none() {
        assert_eq!(quest_task(task_quest(TaskId(5))), Some(TaskId(5)));
        assert_eq!(quest_task(CREW_QUEST), None);
        assert_eq!(quest_task(7), None);
    }
}
