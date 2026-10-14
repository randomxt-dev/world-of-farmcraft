//! The match the world hosts: among-rules' [`Game`], fed by world entry, movement, the clock and
//! the HUD's addon requests, its notices sent back as addon messages under [`wire::PREFIX`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use among_rules::wire::{self, Notice, Request, To};
use among_rules::{
    Actions, Command, Game, Layout, PlayerId, Position, Reject, SabotageDef, SabotageKind,
    Settings, TaskDef, TaskKind,
};
use rand::RngCore;

use crate::world::{packets, player, Map};

/// The one match; cheap to clone. Lock order: the host, then the map.
#[derive(Clone)]
pub struct GameHost(Arc<Mutex<Inner>>);

struct Inner {
    game: Game,
    map: Map,
    /// The buttons each player was last told are live.
    actions: HashMap<PlayerId, Actions>,
}

impl GameHost {
    pub fn new(settings: Settings, layout: Layout, map: Map) -> Self {
        Self(Arc::new(Mutex::new(Inner {
            game: Game::new(settings, layout),
            map,
            actions: HashMap::new(),
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A character entered the world: seat it in the lobby, or refuse while a game runs.
    pub fn join(&self, guid: u64) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        if let Err(r) = inner.apply(Command::Join { player }) {
            inner.notify(To::Player(player), &Notice::Refused(r.to_string()));
        }
        let host = inner.game.host();
        inner.notify(To::Player(player), &Notice::Host(host));
    }

    /// A character left the world.
    pub fn leave(&self, guid: u64) {
        let mut inner = self.lock();
        let _ = inner.apply(Command::Leave {
            player: PlayerId(guid),
        });
        inner.actions.remove(&PlayerId(guid));
    }

    /// A character moved; spectators outside the game are ignored.
    pub fn moved(&self, guid: u64, position: Position) {
        let _ = self.lock().apply(Command::Move {
            player: PlayerId(guid),
            position,
        });
    }

    /// A HUD button press; a refusal goes back to the presser with its reason.
    pub fn request(&self, guid: u64, request: Request) {
        let mut inner = self.lock();
        let player = PlayerId(guid);
        let result = inner
            .command(player, request)
            .and_then(|cmd| inner.apply(cmd));
        if let Err(r) = result {
            tracing::debug!("game: {guid} {request:?} refused: {r}");
            inner.notify(To::Player(player), &Notice::Refused(r.to_string()));
        }
    }

    /// An addon payload that is not a [`Request`].
    pub fn unknown_request(&self, guid: u64, payload: &str) {
        tracing::debug!("game: {guid} sent an unknown request {payload:?}");
        self.lock().notify(
            To::Player(PlayerId(guid)),
            &Notice::Refused("unknown request".into()),
        );
    }

    pub fn tick(&self, dt: Duration) {
        let _ = self.lock().apply(Command::Tick { dt });
    }
}

impl Inner {
    /// Apply `cmd`, send what its events tell whom, then any changed buttons.
    fn apply(&mut self, cmd: Command) -> Result<(), Reject> {
        let events = self.game.apply(cmd)?;
        for event in &events {
            tracing::debug!("game: {event:?}");
            for (to, notice) in wire::notices(event, &self.game) {
                self.notify(to, &notice);
            }
        }
        self.refresh_actions();
        Ok(())
    }

    /// The command a press means now. Kill, report and use aim at the nearest legal target; with
    /// none, the refusal names the first rule in the way, or "nothing in reach".
    fn command(&self, player: PlayerId, request: Request) -> Result<Command, Reject> {
        let g = &self.game;
        let none = |probe: Command| match g.validate(&probe) {
            Err(Reject::InvalidTarget | Reject::NoSuchBody) | Ok(()) => Reject::OutOfRange,
            Err(r) => r,
        };
        Ok(match request {
            Request::Start => Command::Start {
                by: player,
                seed: rand::thread_rng().next_u64(),
            },
            Request::Kill => match g.kill_target(player) {
                Some(victim) => Command::Kill {
                    killer: player,
                    victim,
                },
                None => {
                    return Err(none(Command::Kill {
                        killer: player,
                        victim: player,
                    }))
                }
            },
            Request::Report => match g.report_target(player) {
                Some(body) => Command::Report { player, body },
                None => {
                    return Err(none(Command::Report {
                        player,
                        body: player,
                    }))
                }
            },
            Request::Use => match g.use_target(player) {
                Some(cmd) => cmd,
                None => return Err(none(Command::CallEmergency { player })),
            },
            Request::Vote(vote) => Command::Vote {
                voter: player,
                vote,
            },
            Request::Sabotage(kind) => Command::Sabotage { player, kind },
            Request::Release => Command::ReleaseFix { player },
        })
    }

    /// Tell every seated player whose live buttons changed.
    fn refresh_actions(&mut self) {
        let changed: Vec<(PlayerId, Actions)> = (self.game.players())
            .filter(|(_, p)| p.connected)
            .map(|(id, _)| (id, self.game.actions(id)))
            .filter(|(id, a)| self.actions.get(id) != Some(a))
            .collect();
        for (id, a) in changed {
            self.actions.insert(id, a);
            self.notify(To::Player(id), &Notice::Actions(a));
        }
    }

    /// Each recipient gets the line under its own guid, which its name query always answers.
    fn notify(&self, to: To, notice: &Notice) {
        let payload = notice.encode();
        let send = |guid: u64| {
            self.map
                .send(guid, packets::addon_message(guid, wire::PREFIX, &payload));
        };
        match to {
            To::Player(p) => send(p.0),
            To::All => self.map.guids().into_iter().for_each(send),
        }
    }
}

/// Stand-in spots around Northshire Abbey's steps until the map is surveyed: the button on the
/// spawn, tasks and stations 8-25 yd out at the spawn's height.
pub fn placeholder_layout() -> Layout {
    let [x, y, z] = player::SPAWN.position;
    let at = |dx: f32, dy: f32| [x + dx, y + dy, z];
    let task = |name: &str, kind, steps: Vec<Position>| TaskDef {
        name: name.into(),
        kind,
        steps,
    };
    let sabotage = |kind, stations, deadline| SabotageDef {
        kind,
        stations,
        deadline,
    };
    let critical = Some(Duration::from_secs(30));
    Layout {
        emergency_button: at(0.0, 0.0),
        tasks: vec![
            task("Light the candles", TaskKind::Common, vec![at(10.0, 0.0)]),
            task("Ring the bell", TaskKind::Common, vec![at(-10.0, 0.0)]),
            task(
                "Carry the hay",
                TaskKind::Long,
                vec![at(20.0, 10.0), at(20.0, -10.0)],
            ),
            task(
                "Copy the tome",
                TaskKind::Long,
                vec![at(-20.0, 10.0), at(-20.0, -10.0)],
            ),
            task("Sweep the steps", TaskKind::Short, vec![at(0.0, 10.0)]),
            task("Feed the horses", TaskKind::Short, vec![at(0.0, -10.0)]),
            task("Fill the trough", TaskKind::Short, vec![at(10.0, 15.0)]),
            task("Check the gate", TaskKind::Short, vec![at(-10.0, -15.0)]),
        ],
        sabotages: vec![
            sabotage(SabotageKind::Lights, vec![at(8.0, 8.0)], None),
            sabotage(SabotageKind::Comms, vec![at(-8.0, 8.0)], None),
            sabotage(
                SabotageKind::Oxygen,
                vec![at(15.0, -20.0), at(-15.0, -20.0)],
                critical,
            ),
            sabotage(
                SabotageKind::Reactor,
                vec![at(25.0, 0.0), at(25.0, 8.0)],
                critical,
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::CharacterRecord;
    use crate::world::Packet;
    use benilla_protocol::messages::{parse_server_with_tail, ServerPacket};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    /// A host on the placeholder map with players `1..=n` in the world at the spawn; each
    /// receiver is that player's outbox.
    fn hosted(settings: Settings, n: u64) -> (GameHost, Vec<UnboundedReceiver<Packet>>) {
        let map = Map::default();
        let host = GameHost::new(settings, placeholder_layout(), map.clone());
        let mut inboxes = Vec::new();
        for guid in 1..=n {
            let (tx, rx) = unbounded_channel();
            let character = CharacterRecord {
                guid,
                name: format!("P{guid}"),
                race: 1,
                class: 1,
                gender: 0,
                skin: 0,
                face: 0,
                hair_style: 0,
                hair_color: 0,
                facial_hair: 0,
                level: 1,
            };
            map.enter(character, tx, player::spawn_living());
            host.join(guid);
            inboxes.push(rx);
        }
        (host, inboxes)
    }

    /// The Among Azeroth notices waiting in `inbox`, decoded; everything else is skipped.
    fn notices(inbox: &mut UnboundedReceiver<Packet>) -> Vec<Notice> {
        let mut out = Vec::new();
        while let Ok(p) = inbox.try_recv() {
            let Ok((ServerPacket::MessageChat(m), _)) = parse_server_with_tail(p.opcode, &p.body)
            else {
                continue;
            };
            let (prefix, payload) = m.text.split_once('\t').unwrap();
            assert_eq!(prefix, wire::PREFIX);
            out.push(Notice::decode(payload).expect(payload));
        }
        out
    }

    fn quick() -> Settings {
        Settings {
            min_players: 3,
            initial_kill_cooldown: Duration::ZERO,
            ..Settings::default()
        }
    }

    #[test]
    fn joiners_learn_the_host_and_only_the_host_starts() {
        let (host, mut inboxes) = hosted(quick(), 3);
        for inbox in &mut inboxes {
            assert!(notices(inbox).contains(&Notice::Host(Some(PlayerId(1)))));
        }
        host.request(2, Request::Start);
        assert_eq!(
            notices(&mut inboxes[1]),
            [Notice::Refused(Reject::NotHost.to_string())]
        );
        host.request(1, Request::Start);
        let all = notices(&mut inboxes[2]);
        assert!(all.contains(&Notice::Started {
            players: 3,
            impostors: 1
        }));
        assert!(all.iter().any(|n| matches!(n, Notice::Role { .. })));
        assert_eq!(
            all.iter()
                .filter(|n| matches!(n, Notice::Task { .. }))
                .count(),
            4
        );
    }

    #[test]
    fn a_kill_press_aims_at_the_nearest_crewmate_and_tells_only_the_victim() {
        let (host, mut inboxes) = hosted(quick(), 3);
        host.request(1, Request::Start);
        let roles: Vec<Vec<Notice>> = inboxes.iter_mut().map(notices).collect();
        let impostor = (1..=3u64)
            .find(|&g| {
                roles[g as usize - 1].iter().any(|n| {
                    matches!(
                        n,
                        Notice::Role {
                            role: among_rules::Role::Impostor,
                            ..
                        }
                    )
                })
            })
            .unwrap();
        // Everyone stands on the spawn, so the impostor's Kill is live.
        assert!(roles[impostor as usize - 1]
            .iter()
            .any(|n| matches!(n, Notice::Actions(Actions { kill: true, .. }))));
        // Crewmates' presses are refused with the rule's reason.
        let crew = (1..=3u64).find(|&g| g != impostor).unwrap();
        host.request(crew, Request::Kill);
        assert_eq!(
            notices(&mut inboxes[crew as usize - 1]),
            [Notice::Refused(Reject::NotImpostor.to_string())]
        );

        host.request(impostor, Request::Kill);
        let told: Vec<(u64, Vec<Notice>)> = (1..=3u64)
            .map(|g| (g, notices(&mut inboxes[g as usize - 1])))
            .collect();
        let victims: Vec<u64> = (told.iter())
            .filter(|(_, n)| n.contains(&Notice::Dead))
            .map(|(g, _)| *g)
            .collect();
        assert_eq!(victims.len(), 1, "{told:?}");
        assert_ne!(victims[0], impostor);
        // One crewmate left against one impostor: the game is over for everyone.
        for (_, n) in &told {
            assert!(n.iter().any(|n| matches!(n, Notice::Over { .. })), "{n:?}");
        }
    }

    #[test]
    fn report_and_vote_presses_run_a_meeting() {
        let (host, mut inboxes) = hosted(
            Settings {
                min_players: 4,
                ..quick()
            },
            4,
        );
        host.request(1, Request::Start);
        let impostor = (1..=4u64)
            .find(|&g| {
                notices(&mut inboxes[g as usize - 1]).iter().any(|n| {
                    matches!(
                        n,
                        Notice::Role {
                            role: among_rules::Role::Impostor,
                            ..
                        }
                    )
                })
            })
            .unwrap();
        host.request(impostor, Request::Kill);
        let reporter = (1..=4u64)
            .find(|&g| {
                g != impostor && !notices(&mut inboxes[g as usize - 1]).contains(&Notice::Dead)
            })
            .unwrap();
        host.request(reporter, Request::Report);
        let seen = notices(&mut inboxes[reporter as usize - 1]);
        assert!(
            seen.iter().any(|n| matches!(n, Notice::Meeting { caller, body: Some(_), .. } if caller.0 == reporter)),
            "{seen:?}"
        );
        // Votes wait for the discussion to end.
        host.request(reporter, Request::Vote(among_rules::Vote::Skip));
        assert_eq!(
            notices(&mut inboxes[reporter as usize - 1]),
            [Notice::Refused(Reject::WrongPhase.to_string())]
        );
        host.tick(Duration::from_secs(15));
        host.request(
            reporter,
            Request::Vote(among_rules::Vote::Player(PlayerId(impostor))),
        );
        let seen = notices(&mut inboxes[reporter as usize - 1]);
        assert!(seen.contains(&Notice::Voting(Duration::from_secs(120))));
        assert!(seen.contains(&Notice::Voted(PlayerId(reporter))));
    }
}
