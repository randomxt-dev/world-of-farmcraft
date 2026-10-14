//! What the stock client reads in its chat frame: each HUD notice as a plain system line, worded
//! for the one player who reads it. A notice the HUD alone needs (live buttons, the task bar's
//! count) has no line.

use among_rules::wire::Notice;
use among_rules::{Game, PlayerId, Role, SabotageKind, TaskId, Team, Vote, VoteOutcome, WinReason};

/// The line `notice` shows `reader`, if any; `name` turns a player into their character's name.
pub fn line(
    notice: &Notice,
    reader: PlayerId,
    game: &Game,
    name: &dyn Fn(PlayerId) -> String,
) -> Option<String> {
    Some(match notice {
        Notice::Host(Some(host)) if *host == reader => {
            "You host this game: sound the horn (Lion Horn on your action bar) to start it.".into()
        }
        Notice::Host(Some(host)) => format!("{} hosts this game.", name(*host)),
        Notice::Host(None) => return None,
        Notice::Started { players, impostors } => format!(
            "The game begins: {players} players, {} among you.",
            count(*impostors, "impostor", "impostors")
        ),
        Notice::Role {
            role: Role::Impostor,
            fellow_impostors,
        } => {
            let mut text =
                "You are an IMPOSTOR. Kill the crew unseen and fake your tasks to blend \
                            in."
                .to_string();
            if !fellow_impostors.is_empty() {
                let names: Vec<String> = fellow_impostors.iter().map(|&p| name(p)).collect();
                text += &format!(" Your fellow impostors: {}.", names.join(", "));
            }
            text
        }
        Notice::Role {
            role: Role::Crewmate,
            ..
        } => "You are a CREWMATE. Finish your tasks and vote the impostors out.".into(),
        Notice::Task {
            task: id,
            name: task,
            step,
            steps,
        } => {
            let next = where_is(game, *id, *step);
            match *step {
                0 => format!("Your task: {task}, {next}."),
                s if s >= *steps => format!("Task done: {task}."),
                s => format!("{task}: step {s} of {steps} done; next {next}."),
            }
        }
        Notice::Slay { kills, count, .. } => format!("Slain: {kills} of {count}."),
        Notice::Gather { picked, count, .. } => format!("Gathered: {picked} of {count}."),
        Notice::Dead => {
            let impostor = game.player(reader).is_some_and(|p| p.is_impostor());
            let after = if impostor {
                "As a ghost you can still sabotage."
            } else {
                "As a ghost you still do your tasks, unseen by the living."
            };
            format!("You are dead. Release your spirit to walk on. {after}")
        }
        Notice::Meeting {
            caller,
            body,
            discussion,
        } => {
            let what = match body {
                Some(body) => format!("{} found {}'s body!", name(*caller), name(*body)),
                None => format!("{} called an emergency meeting!", name(*caller)),
            };
            format!("{what} Discuss for {} s, then vote.", discussion.as_secs())
        }
        Notice::Voting(time) => format!(
            "Voting is open for {} s: pick in the vote window, or type .vote <name> or .vote skip.",
            time.as_secs()
        ),
        Notice::Voted(voter) => format!("{} has voted.", name(*voter)),
        Notice::Unvoted(voter) => {
            format!("{}'s vote came back: they may vote again.", name(*voter))
        }
        Notice::Tally { outcome, votes } => {
            let mut lines: Vec<String> = votes
                .iter()
                .map(|(voter, vote)| match vote {
                    Vote::Player(p) => format!("{} voted {}", name(*voter), name(*p)),
                    Vote::Skip => format!("{} skipped", name(*voter)),
                })
                .collect();
            if lines.is_empty() {
                lines.push("nobody voted".into());
            }
            let result = match outcome {
                VoteOutcome::Ejected(_) => return Some(format!("Votes: {}.", lines.join(", "))),
                VoteOutcome::Tie => "A tie: nobody is ejected.",
                VoteOutcome::Skipped => "Skipped: nobody is ejected.",
            };
            format!("Votes: {}. {result}", lines.join(", "))
        }
        Notice::Ejected {
            player,
            impostor,
            impostors_left,
        } => {
            let who = name(*player);
            let mut text = format!("{who} was ejected.");
            match impostor {
                Some(true) => text += &format!(" {who} was an impostor."),
                Some(false) => text += &format!(" {who} was not an impostor."),
                None => {}
            }
            if let Some(left) = impostors_left {
                text += &format!(" {} remain.", count(*left, "impostor", "impostors"));
            }
            text
        }
        Notice::Resume => "The meeting is over: back to work.".into(),
        Notice::Sabotage { kind, deadline } => {
            let what = match kind {
                SabotageKind::Lights => "the lights are out",
                SabotageKind::Comms => "communications are down",
                SabotageKind::Oxygen => "the oxygen is failing",
                SabotageKind::Reactor => "the reactor is melting down",
            };
            match deadline {
                Some(d) => format!(
                    "Sabotage: {what}! Fix it within {} s or the impostors win.",
                    d.as_secs()
                ),
                None => format!("Sabotage: {what}! Fix it at its stations."),
            }
        }
        Notice::Fixed(kind) => format!("{kind:?} is fixed."),
        Notice::Over {
            winner,
            reason,
            impostors,
        } => {
            let who = match winner {
                Team::Crew => "The crew wins",
                Team::Impostors => "The impostors win",
            };
            let why = match reason {
                WinReason::TasksCompleted => "every task is done",
                WinReason::ImpostorsEliminated => "no impostor is left",
                WinReason::Outnumbered => "the impostors match the living crew",
                WinReason::Sabotage => "a sabotage was not fixed in time",
            };
            let names: Vec<String> = impostors.iter().map(|&p| name(p)).collect();
            let host = game.host().map_or("The host".into(), name);
            format!(
                "{who}: {why}. The impostors were {}. {host} can sound the horn to return to \
                 the lobby.",
                names.join(", ")
            )
        }
        Notice::Lobby => {
            let host = game.host().map_or("The host".into(), name);
            format!("Back in the lobby. {host} sounds the horn to start the next game.")
        }
        // The crew's total, told to everyone as it grows; zero is the start's, not news.
        Notice::TaskBar { done, total } if *done > 0 => {
            format!("Crew tasks done: {done} of {total}.")
        }
        Notice::TaskBar { .. }
        | Notice::Actions(_)
        | Notice::Refused(_)
        | Notice::Station { .. } => return None,
    })
}

/// The mid-screen headline `notice` also raises for `reader`, for the moments that matter most.
pub fn headline(notice: &Notice, name: &dyn Fn(PlayerId) -> String) -> Option<String> {
    Some(match notice {
        Notice::Role {
            role: Role::Impostor,
            ..
        } => "You are an Impostor".into(),
        Notice::Role {
            role: Role::Crewmate,
            ..
        } => "You are a Crewmate".into(),
        Notice::Dead => "You died".into(),
        Notice::Ejected { player, .. } => format!("{} was ejected", name(*player)),
        Notice::Sabotage { .. } => "Sabotage!".into(),
        Notice::Over {
            winner: Team::Crew, ..
        } => "The crew wins!".into(),
        Notice::Over {
            winner: Team::Impostors,
            ..
        } => "The impostors win!".into(),
        _ => return None,
    })
}

/// Where `task`'s step `step` is worked, from the meeting table: "75 yd west of the table".
fn where_is(game: &Game, task: TaskId, step: usize) -> String {
    let layout = game.layout();
    let Some(s) = layout.tasks.get(task.0).and_then(|t| t.steps.get(step)) else {
        return "somewhere".into();
    };
    let from = layout.emergency_button;
    // WoW's world axes: +x is north, +y is west.
    let (north, west) = (s.at[0] - from[0], s.at[1] - from[1]);
    let yards = (north * north + west * west).sqrt().round();
    if yards < 5.0 {
        return "at the table".into();
    }
    const POINTS: [&str; 8] = [
        "north",
        "northwest",
        "west",
        "southwest",
        "south",
        "southeast",
        "east",
        "northeast",
    ];
    let eighth = (west.atan2(north) / std::f32::consts::FRAC_PI_4).round() as i32;
    format!(
        "{yards} yd {} of the table",
        POINTS[eighth.rem_euclid(8) as usize]
    )
}

/// `n` and the noun that fits it: "1 impostor", "2 impostors".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;
    use among_rules::{Layout, Settings, TaskDef, TaskKind, TaskStep};

    #[test]
    fn a_task_spot_is_told_as_yards_and_a_compass_point_from_the_table() {
        let spots = [
            [0.0, 75.0, 0.0],
            [40.0, 0.0, 3.0],
            [-30.0, -30.0, 0.0],
            [2.0, 1.0, 0.0],
        ];
        let layout = Layout {
            emergency_button: [0.0; 3],
            tasks: vec![TaskDef {
                name: "Walk".into(),
                kind: TaskKind::Short,
                steps: spots.iter().map(|&at| TaskStep::cast(at, 1.0)).collect(),
            }],
            sabotages: Vec::new(),
            creatures: Vec::new(),
        };
        let game = Game::new(Settings::default(), layout);
        let told: Vec<String> = (0..4).map(|s| where_is(&game, TaskId(0), s)).collect();
        assert_eq!(
            told,
            [
                "75 yd west of the table",
                "40 yd north of the table",
                "42 yd southeast of the table",
                "at the table"
            ]
        );
    }
}
