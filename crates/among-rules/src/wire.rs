//! The words the HUD and the server exchange. 1.12 has no addon opcode: an addon message is a chat
//! line in `LANG_ADDON` whose text is a prefix, a TAB, then the payload (benilla-protocol
//! `LANGUAGE_ADDON`). Ours is [`PREFIX`]; a payload is space-separated words, guids in decimal,
//! durations in milliseconds, a free-text name only as the last field.

use std::time::Duration;

use crate::game::{Actions, Game, Station};
use crate::protocol::{
    Event, MeetingReason, PlayerId, Role, TaskId, Team, Vote, VoteOutcome, WinReason,
};
use crate::settings::SabotageKind;

/// The addon prefix of every Among Azeroth message.
pub const PREFIX: &str = "AMONG";

/// A button press, HUD to server. The server picks the target (nearest legal one) for kill,
/// report and use, so a client cannot aim past what the rules allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Start,
    Use,
    Kill,
    Report,
    Vote(Vote),
    Sabotage(SabotageKind),
    /// Let go of a held sabotage station.
    Release,
}

/// What the server tells one player.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Host(Option<PlayerId>),
    Started {
        players: usize,
        impostors: usize,
    },
    Role {
        role: Role,
        fellow_impostors: Vec<PlayerId>,
    },
    /// One task of yours and its progress; sent at the start and on every step.
    Task {
        task: TaskId,
        step: usize,
        steps: usize,
        name: String,
    },
    TaskBar {
        done: usize,
        total: usize,
    },
    /// You were killed.
    Dead,
    /// Your live buttons; sent when they change.
    Actions(Actions),
    /// Your last request was refused, and why.
    Refused(String),
    Meeting {
        caller: PlayerId,
        /// The reported body; `None` for an emergency meeting.
        body: Option<PlayerId>,
        discussion: Duration,
    },
    Voting(Duration),
    Voted(PlayerId),
    /// A vote for a player who left came back; the voter may vote again.
    Unvoted(PlayerId),
    Tally {
        outcome: VoteOutcome,
        votes: Vec<(PlayerId, Vote)>,
    },
    Ejected {
        player: PlayerId,
        impostor: Option<bool>,
        impostors_left: Option<usize>,
    },
    /// The meeting is over; play resumes unless [`Notice::Over`] follows.
    Resume,
    Sabotage {
        kind: SabotageKind,
        deadline: Option<Duration>,
    },
    Station {
        station: usize,
        state: Station,
    },
    Fixed(SabotageKind),
    Over {
        winner: Team,
        reason: WinReason,
        impostors: Vec<PlayerId>,
    },
    Lobby,
}

/// Who a notice is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    All,
    Player(PlayerId),
}

/// What `event` tells whom. `Killed` reaches only the victim: the killer stays secret and the
/// world shows the body. Bodies clearing and joins are the world's to show, not the HUD's.
pub fn notices(event: &Event, game: &Game) -> Vec<(To, Notice)> {
    let all = |n| vec![(To::All, n)];
    match event {
        Event::Joined { .. } | Event::Left { .. } | Event::BodiesCleared => Vec::new(),
        Event::SettingsChanged | Event::TaskCompleted { .. } => Vec::new(),
        Event::HostChanged { host } => all(Notice::Host(*host)),
        Event::GameStarted { players, impostors } => all(Notice::Started {
            players: *players,
            impostors: *impostors,
        }),
        Event::RoleAssigned {
            player,
            role,
            fellow_impostors,
        } => vec![(
            To::Player(*player),
            Notice::Role {
                role: *role,
                fellow_impostors: fellow_impostors.clone(),
            },
        )],
        Event::TasksAssigned { player, tasks } => (tasks.iter())
            .map(|&task| (To::Player(*player), task_notice(game, task, 0)))
            .collect(),
        Event::TaskProgress {
            player, task, step, ..
        } => vec![(To::Player(*player), task_notice(game, *task, *step))],
        Event::TaskBar { done, total } => all(Notice::TaskBar {
            done: *done,
            total: *total,
        }),
        Event::Killed { victim, .. } => vec![(To::Player(*victim), Notice::Dead)],
        Event::MeetingCalled {
            caller,
            reason,
            discussion,
        } => all(Notice::Meeting {
            caller: *caller,
            body: match reason {
                MeetingReason::Report { body } => Some(*body),
                MeetingReason::Emergency => None,
            },
            discussion: *discussion,
        }),
        Event::VotingOpened { voting } => all(Notice::Voting(*voting)),
        Event::VoteCast { voter } => all(Notice::Voted(*voter)),
        Event::VoteReturned { voter } => all(Notice::Unvoted(*voter)),
        Event::VotingClosed { votes, outcome } => all(Notice::Tally {
            outcome: *outcome,
            votes: votes.clone(),
        }),
        Event::Ejected {
            player,
            impostor,
            impostors_left,
        } => all(Notice::Ejected {
            player: *player,
            impostor: *impostor,
            impostors_left: *impostors_left,
        }),
        Event::MeetingEnded => all(Notice::Resume),
        Event::SabotageStarted { kind, deadline } => all(Notice::Sabotage {
            kind: *kind,
            deadline: *deadline,
        }),
        Event::StationHeld { station, player } => all(Notice::Station {
            station: *station,
            state: Station::Held(*player),
        }),
        Event::StationReleased { station } => all(Notice::Station {
            station: *station,
            state: Station::Open,
        }),
        Event::StationFixed { station, .. } => all(Notice::Station {
            station: *station,
            state: Station::Fixed,
        }),
        Event::SabotageFixed { kind } => all(Notice::Fixed(*kind)),
        Event::GameOver {
            winner,
            reason,
            impostors,
        } => all(Notice::Over {
            winner: *winner,
            reason: *reason,
            impostors: impostors.clone(),
        }),
        Event::ReturnedToLobby => all(Notice::Lobby),
    }
}

fn task_notice(game: &Game, task: TaskId, step: usize) -> Notice {
    let def = &game.layout().tasks[task.0];
    Notice::Task {
        task,
        step,
        steps: def.steps.len(),
        name: def.name.clone(),
    }
}

impl Request {
    pub fn encode(&self) -> String {
        match self {
            Request::Start => "start".into(),
            Request::Use => "use".into(),
            Request::Kill => "kill".into(),
            Request::Report => "report".into(),
            Request::Vote(vote) => format!("vote {}", vote_word(*vote)),
            Request::Sabotage(kind) => format!("sabotage {}", sabotage_word(*kind)),
            Request::Release => "release".into(),
        }
    }

    pub fn decode(payload: &str) -> Option<Self> {
        let mut w = payload.split(' ');
        let request = match w.next()? {
            "start" => Request::Start,
            "use" => Request::Use,
            "kill" => Request::Kill,
            "report" => Request::Report,
            "vote" => Request::Vote(parse_vote(w.next()?)?),
            "sabotage" => Request::Sabotage(parse_sabotage(w.next()?)?),
            "release" => Request::Release,
            _ => return None,
        };
        w.next().is_none().then_some(request)
    }
}

impl Notice {
    pub fn encode(&self) -> String {
        match self {
            Notice::Host(host) => format!("host {}", opt_guid(*host)),
            Notice::Started { players, impostors } => format!("started {players} {impostors}"),
            Notice::Role {
                role,
                fellow_impostors,
            } => match role {
                Role::Crewmate => "role crew".into(),
                Role::Impostor => format!("role impostor {}", guids(fellow_impostors)),
            },
            Notice::Task {
                task,
                step,
                steps,
                name,
            } => format!("task {} {step} {steps} {name}", task.0),
            Notice::TaskBar { done, total } => format!("bar {done} {total}"),
            Notice::Dead => "dead".into(),
            Notice::Actions(a) => format!(
                "actions {} {} {} {} {}",
                u8::from(a.kill),
                u8::from(a.report),
                u8::from(a.use_),
                u8::from(a.sabotage),
                a.kill_cooldown_secs
            ),
            Notice::Refused(why) => format!("refused {why}"),
            Notice::Meeting {
                caller,
                body,
                discussion,
            } => format!(
                "meeting {} {} {}",
                caller.0,
                opt_guid(*body),
                discussion.as_millis()
            ),
            Notice::Voting(time) => format!("voting {}", time.as_millis()),
            Notice::Voted(p) => format!("voted {}", p.0),
            Notice::Unvoted(p) => format!("unvoted {}", p.0),
            Notice::Tally { outcome, votes } => {
                let outcome = match outcome {
                    VoteOutcome::Ejected(p) => format!("eject {}", p.0),
                    VoteOutcome::Tie => "tie 0".into(),
                    VoteOutcome::Skipped => "skip 0".into(),
                };
                let votes: Vec<String> = (votes.iter())
                    .map(|(voter, vote)| format!("{}:{}", voter.0, vote_word(*vote)))
                    .collect();
                format!("tally {outcome} {}", list(&votes))
            }
            Notice::Ejected {
                player,
                impostor,
                impostors_left,
            } => format!(
                "ejected {} {} {}",
                player.0,
                impostor.map_or("?".into(), |i| u8::from(i).to_string()),
                impostors_left.map_or("?".into(), |n| n.to_string())
            ),
            Notice::Resume => "resume".into(),
            Notice::Sabotage { kind, deadline } => format!(
                "sabotage {} {}",
                sabotage_word(*kind),
                deadline.map_or("-".into(), |d| d.as_millis().to_string())
            ),
            Notice::Station { station, state } => match state {
                Station::Open => format!("station {station} open"),
                Station::Held(p) => format!("station {station} held {}", p.0),
                Station::Fixed => format!("station {station} fixed"),
            },
            Notice::Fixed(kind) => format!("fixed {}", sabotage_word(*kind)),
            Notice::Over {
                winner,
                reason,
                impostors,
            } => format!(
                "over {} {} {}",
                match winner {
                    Team::Crew => "crew",
                    Team::Impostors => "impostors",
                },
                match reason {
                    WinReason::TasksCompleted => "tasks",
                    WinReason::ImpostorsEliminated => "eliminated",
                    WinReason::Outnumbered => "outnumbered",
                    WinReason::Sabotage => "sabotage",
                },
                guids(impostors)
            ),
            Notice::Lobby => "lobby".into(),
        }
    }

    pub fn decode(payload: &str) -> Option<Self> {
        let (verb, rest) = payload.split_once(' ').unwrap_or((payload, ""));
        let mut w = rest.split(' ');
        let mut next = || w.next();
        let notice = match verb {
            "host" => Notice::Host(parse_opt_guid(next()?)?),
            "started" => Notice::Started {
                players: next()?.parse().ok()?,
                impostors: next()?.parse().ok()?,
            },
            "role" => match next()? {
                "crew" => Notice::Role {
                    role: Role::Crewmate,
                    fellow_impostors: Vec::new(),
                },
                "impostor" => Notice::Role {
                    role: Role::Impostor,
                    fellow_impostors: parse_guids(next()?)?,
                },
                _ => return None,
            },
            "task" => {
                // The name is the rest of the line and may hold spaces.
                let mut f = rest.splitn(4, ' ');
                return Some(Notice::Task {
                    task: TaskId(f.next()?.parse().ok()?),
                    step: f.next()?.parse().ok()?,
                    steps: f.next()?.parse().ok()?,
                    name: f.next()?.to_owned(),
                });
            }
            "bar" => Notice::TaskBar {
                done: next()?.parse().ok()?,
                total: next()?.parse().ok()?,
            },
            "dead" => Notice::Dead,
            "actions" => {
                let mut flag = || match next()? {
                    "0" => Some(false),
                    "1" => Some(true),
                    _ => None,
                };
                Notice::Actions(Actions {
                    kill: flag()?,
                    report: flag()?,
                    use_: flag()?,
                    sabotage: flag()?,
                    kill_cooldown_secs: next()?.parse().ok()?,
                })
            }
            "refused" => return Some(Notice::Refused(rest.to_owned())),
            "meeting" => Notice::Meeting {
                caller: parse_guid(next()?)?,
                body: parse_opt_guid(next()?)?,
                discussion: parse_millis(next()?)?,
            },
            "voting" => Notice::Voting(parse_millis(next()?)?),
            "voted" => Notice::Voted(parse_guid(next()?)?),
            "unvoted" => Notice::Unvoted(parse_guid(next()?)?),
            "tally" => {
                let outcome = match (next()?, next()?) {
                    ("eject", p) => VoteOutcome::Ejected(parse_guid(p)?),
                    ("tie", "0") => VoteOutcome::Tie,
                    ("skip", "0") => VoteOutcome::Skipped,
                    _ => return None,
                };
                let votes = (split_list(next()?).iter())
                    .map(|v| {
                        let (voter, vote) = v.split_once(':')?;
                        Some((parse_guid(voter)?, parse_vote(vote)?))
                    })
                    .collect::<Option<_>>()?;
                Notice::Tally { outcome, votes }
            }
            "ejected" => Notice::Ejected {
                player: parse_guid(next()?)?,
                impostor: match next()? {
                    "?" => None,
                    "0" => Some(false),
                    "1" => Some(true),
                    _ => return None,
                },
                impostors_left: match next()? {
                    "?" => None,
                    n => Some(n.parse().ok()?),
                },
            },
            "resume" => Notice::Resume,
            "sabotage" => Notice::Sabotage {
                kind: parse_sabotage(next()?)?,
                deadline: match next()? {
                    "-" => None,
                    ms => Some(parse_millis(ms)?),
                },
            },
            "station" => Notice::Station {
                station: next()?.parse().ok()?,
                state: match next()? {
                    "open" => Station::Open,
                    "fixed" => Station::Fixed,
                    "held" => Station::Held(parse_guid(next()?)?),
                    _ => return None,
                },
            },
            "fixed" => Notice::Fixed(parse_sabotage(next()?)?),
            "over" => Notice::Over {
                winner: match next()? {
                    "crew" => Team::Crew,
                    "impostors" => Team::Impostors,
                    _ => return None,
                },
                reason: match next()? {
                    "tasks" => WinReason::TasksCompleted,
                    "eliminated" => WinReason::ImpostorsEliminated,
                    "outnumbered" => WinReason::Outnumbered,
                    "sabotage" => WinReason::Sabotage,
                    _ => return None,
                },
                impostors: parse_guids(next()?)?,
            },
            "lobby" => Notice::Lobby,
            _ => return None,
        };
        // Nothing may trail a fixed-arity notice; "dead", "resume" and "lobby" have no fields.
        let trailing = w.next().is_some_and(|s| !s.is_empty());
        (!trailing).then_some(notice)
    }
}

fn vote_word(vote: Vote) -> String {
    match vote {
        Vote::Skip => "skip".into(),
        Vote::Player(p) => p.0.to_string(),
    }
}

fn parse_vote(word: &str) -> Option<Vote> {
    match word {
        "skip" => Some(Vote::Skip),
        g => parse_guid(g).map(Vote::Player),
    }
}

fn sabotage_word(kind: SabotageKind) -> &'static str {
    match kind {
        SabotageKind::Lights => "lights",
        SabotageKind::Comms => "comms",
        SabotageKind::Oxygen => "oxygen",
        SabotageKind::Reactor => "reactor",
    }
}

fn parse_sabotage(word: &str) -> Option<SabotageKind> {
    match word {
        "lights" => Some(SabotageKind::Lights),
        "comms" => Some(SabotageKind::Comms),
        "oxygen" => Some(SabotageKind::Oxygen),
        "reactor" => Some(SabotageKind::Reactor),
        _ => None,
    }
}

fn parse_guid(word: &str) -> Option<PlayerId> {
    word.parse().ok().filter(|&g| g != 0).map(PlayerId)
}

/// `0` is "none": no WoW object has guid 0.
fn opt_guid(p: Option<PlayerId>) -> String {
    p.map_or(0, |p| p.0).to_string()
}

fn parse_opt_guid(word: &str) -> Option<Option<PlayerId>> {
    match word.parse::<u64>().ok()? {
        0 => Some(None),
        g => Some(Some(PlayerId(g))),
    }
}

fn parse_millis(word: &str) -> Option<Duration> {
    word.parse().ok().map(Duration::from_millis)
}

/// A comma list, `-` when empty, so it always fills exactly one word.
fn list(items: &[String]) -> String {
    if items.is_empty() {
        "-".into()
    } else {
        items.join(",")
    }
}

fn split_list(word: &str) -> Vec<&str> {
    if word == "-" {
        Vec::new()
    } else {
        word.split(',').collect()
    }
}

fn guids(ids: &[PlayerId]) -> String {
    list(&ids.iter().map(|p| p.0.to_string()).collect::<Vec<_>>())
}

fn parse_guids(word: &str) -> Option<Vec<PlayerId>> {
    split_list(word).into_iter().map(parse_guid).collect()
}
