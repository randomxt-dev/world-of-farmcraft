//! One world connection: the auth handshake, then the roster and the world until the socket
//! closes.

use anyhow::{bail, Context, Result};
use benilla_protocol::messages::{self, opcode};
use benilla_protocol::wire::{read_cstring, read_u32_le, read_u64_le};
use benilla_srp::vanilla_header::HeaderCrypto;
use rand::RngCore;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use among_rules::wire::{self, Request};

use super::map::Map;
use super::packets;
use super::player;
use super::socket::{self, Outbox, Packet};
use super::update;
use crate::accounts::{Accounts, CharacterRecord, CHAR_CREATE_ERROR};
use crate::game::GameHost;
use crate::srp::world_proof;
use crate::survey::{Mark, Survey};

/// `CHAR_DELETE_FAILED` (vmangos `SharedDefines.h`).
const CHAR_DELETE_FAILED: u8 = 0x3A;

pub async fn run(
    stream: TcpStream,
    accounts: Accounts,
    map: Map,
    host: GameHost,
    survey: Survey,
) -> Result<()> {
    stream.set_nodelay(true)?;
    let (mut r, mut w) = stream.into_split();

    let server_seed = rand::thread_rng().next_u32();
    socket::write_plain(&mut w, &packets::auth_challenge(server_seed)).await?;

    // CMSG_AUTH_SESSION, plain: build, server id, account, client seed, digest, addon block.
    let (op, body) = socket::read_packet(&mut r, None).await?;
    if op != opcode::CMSG_AUTH_SESSION {
        bail!("expected CMSG_AUTH_SESSION, got {op:#06x}");
    }
    let mut cur = body.as_slice();
    let build = read_u32_le(&mut cur)?;
    let _server_id = read_u32_le(&mut cur)?;
    let account = read_cstring(&mut cur)?.to_ascii_uppercase();
    let client_seed = read_u32_le(&mut cur)?;
    let digest = benilla_protocol::wire::read_array::<20>(&mut cur)?;

    if build != u32::from(benilla_protocol::CLIENT_BUILD) {
        let refusal = packets::auth_refused(messages::AUTH_VERSION_MISMATCH);
        socket::write_plain(&mut w, &refusal).await?;
        bail!("{account}: build {build} refused");
    }
    let Some(key) = accounts.session_key(&account) else {
        socket::write_plain(
            &mut w,
            &packets::auth_refused(messages::AUTH_UNKNOWN_ACCOUNT),
        )
        .await?;
        bail!("{account}: no realmd logon");
    };
    if world_proof(&account, client_seed, server_seed, &key) != digest {
        socket::write_plain(&mut w, &packets::auth_refused(messages::AUTH_FAILED)).await?;
        bail!("{account}: session proof mismatch");
    }

    let (encrypter, mut decrypter) = HeaderCrypto::from_session_key(key).split();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(socket::run_writer(w, encrypter, rx));
    tx.send(packets::auth_ok())?;
    tracing::info!("world: {account} authenticated");

    let mut session = Session {
        account,
        accounts,
        map,
        host,
        survey,
        outbox: tx,
        in_world: None,
    };
    let result = async {
        loop {
            let (op, body) = socket::read_packet(&mut r, Some(&mut decrypter)).await?;
            session.handle(op, &body)?;
        }
    }
    .await;
    session.leave_world();
    // A closed socket is the ordinary end of a session.
    match result {
        Err(e) if is_disconnect(&e) => Ok(()),
        other => other,
    }
}

fn is_disconnect(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
            )
        })
    })
}

struct Session {
    account: String,
    accounts: Accounts,
    map: Map,
    host: GameHost,
    survey: Survey,
    outbox: Outbox,
    /// The character in the world, once `CMSG_PLAYER_LOGIN` seated it.
    in_world: Option<CharacterRecord>,
}

impl Session {
    fn send(&self, packet: Packet) -> Result<()> {
        self.outbox.send(packet).context("the writer is gone")
    }

    fn handle(&mut self, op: u16, body: &[u8]) -> Result<()> {
        let mut cur = body;
        match op {
            opcode::CMSG_CHAR_ENUM => {
                let roster = self.accounts.characters(&self.account);
                self.send(packets::char_enum(&roster, player::SPAWN))?;
            }
            opcode::CMSG_CHAR_CREATE => {
                let name = read_cstring(&mut cur)?;
                let b = benilla_protocol::wire::read_array::<9>(&mut cur)?;
                let record = CharacterRecord {
                    guid: 0,
                    name,
                    race: b[0],
                    class: b[1],
                    gender: b[2],
                    skin: b[3],
                    face: b[4],
                    hair_style: b[5],
                    hair_color: b[6],
                    facial_hair: b[7],
                    level: 1,
                };
                let code = if record.name.is_empty() {
                    CHAR_CREATE_ERROR
                } else {
                    match self.accounts.create_character(&self.account, record) {
                        Ok(()) => messages::CHAR_CREATE_SUCCESS,
                        Err(code) => code,
                    }
                };
                self.send(packets::char_create_result(code))?;
            }
            opcode::CMSG_CHAR_DELETE => {
                let guid = read_u64_le(&mut cur)?;
                let code = if self.accounts.delete_character(&self.account, guid) {
                    messages::CHAR_DELETE_SUCCESS
                } else {
                    CHAR_DELETE_FAILED
                };
                self.send(packets::char_delete_result(code))?;
            }
            opcode::CMSG_PLAYER_LOGIN => {
                let guid = read_u64_le(&mut cur)?;
                let Some(character) = self.accounts.character(&self.account, guid) else {
                    // `CHAR_LOGIN_NO_CHARACTER`.
                    self.send(Packet::new(opcode::SMSG_CHARACTER_LOGIN_FAILED, vec![0x0D]))?;
                    return Ok(());
                };
                self.leave_world();
                tracing::info!("world: {} enters as {}", self.account, character.name);
                for packet in player::login_packets(&character) {
                    self.send(packet)?;
                }
                self.map.enter(
                    character.clone(),
                    self.outbox.clone(),
                    player::spawn_living(),
                );
                self.host.join(character.guid, character.class);
                self.in_world = Some(character);
            }
            opcode::CMSG_PING => {
                let sequence = read_u32_le(&mut cur)?;
                self.send(packets::pong(sequence))?;
            }
            opcode::CMSG_QUERY_TIME => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as u32);
                self.send(packets::query_time_response(now))?;
            }
            opcode::CMSG_NAME_QUERY => {
                let guid = read_u64_le(&mut cur)?;
                let known = self
                    .map
                    .character(guid)
                    .or_else(|| self.accounts.character(&self.account, guid));
                if let Some(c) = known {
                    self.send(packets::name_query_response(&c))?;
                }
            }
            opcode::CMSG_LOGOUT_REQUEST => {
                // Instant: `u32` result 0, `u8` instant 1, then the completion.
                self.send(Packet::new(
                    opcode::SMSG_LOGOUT_RESPONSE,
                    vec![0, 0, 0, 0, 1],
                ))?;
                self.leave_world();
                self.send(Packet::new(opcode::SMSG_LOGOUT_COMPLETE, Vec::new()))?;
            }
            op if player::is_client_move(op) => {
                if let Some(me) = &self.in_world {
                    let pose = player::read_pose(body);
                    self.map.relay_move(me.guid, op, body, pose);
                    if let Some(pose) = pose {
                        self.host.moved(me.guid, pose.position);
                    }
                }
            }
            opcode::CMSG_CAST_SPELL => {
                if let Some(me) = &self.in_world {
                    let cast = super::spells::CastRequest::read(body)?;
                    self.host.cast(me.guid, cast);
                }
            }
            opcode::CMSG_ITEM_QUERY_SINGLE => {
                let entry = read_u32_le(&mut cur)?;
                self.send(super::items::query_response(entry))?;
            }
            opcode::CMSG_CREATURE_QUERY => {
                let entry = read_u32_le(&mut cur)?;
                self.send(self.host.creature_query(entry))?;
            }
            opcode::CMSG_ATTACKSWING => {
                if let Some(me) = &self.in_world {
                    self.host.attack(me.guid, read_u64_le(&mut cur)?);
                }
            }
            opcode::CMSG_ATTACKSTOP => {
                if let Some(me) = &self.in_world {
                    self.host.stop_attack(me.guid);
                }
            }
            opcode::CMSG_REPOP_REQUEST => {
                if let Some(me) = &self.in_world {
                    self.host.release_spirit(me.guid);
                }
            }
            // A ghost here has no corpse run, so the client draws no marker and offers no
            // Resurrect.
            opcode::MSG_CORPSE_QUERY => self.send(super::death::corpse_not_found())?,
            // The vote menu: the voter picks a line of their own ballot.
            opcode::CMSG_GOSSIP_SELECT_OPTION => {
                if let (Some(me), Some((menu, option))) =
                    (&self.in_world, super::meeting::read_choice(body))
                {
                    self.host.choose(me.guid, menu, option);
                }
            }
            opcode::CMSG_NPC_TEXT_QUERY => {
                if read_u32_le(&mut cur)? == super::meeting::VOTE_TEXT {
                    self.send(super::meeting::vote_text())?;
                }
            }
            opcode::CMSG_QUEST_QUERY => {
                if let Some(template) = self.host.quest_query(read_u32_le(&mut cur)?) {
                    self.send(template)?;
                }
            }
            opcode::CMSG_GAMEOBJECT_QUERY => {
                let entry = read_u32_le(&mut cur)?;
                self.send(self.host.gameobject_query(entry))?;
            }
            opcode::CMSG_GAMEOBJ_USE => {
                if let Some(me) = &self.in_world {
                    self.host.use_object(me.guid, read_u64_le(&mut cur)?);
                }
            }
            opcode::CMSG_MESSAGECHAT => {
                let chat = packets::ClientChat::read(body)?;
                match (&self.in_world, chat.addon_payload(wire::PREFIX)) {
                    (Some(me), Some(payload)) => match Request::decode(payload) {
                        Some(request) => self.host.request(me.guid, request),
                        None => self.host.unknown_request(me.guid, payload),
                    },
                    (Some(me), None) if chat.text.starts_with('.') => {
                        self.dot_command(me.guid, &chat.text)?
                    }
                    (Some(me), None) => super::chat::relay(&self.map, me.guid, &chat),
                    _ => tracing::debug!(
                        "world: {} said {:?} out of the world",
                        self.account,
                        chat.text
                    ),
                }
            }
            other => {
                tracing::debug!(
                    "world: {} sent {} ({other:#06x}, {} bytes), ignored",
                    self.account,
                    messages::opcode_name(other).unwrap_or("?"),
                    body.len()
                );
            }
        }
        Ok(())
    }

    /// A `.` command typed in chat: the survey's, answered as system lines.
    fn dot_command(&self, guid: u64, text: &str) -> Result<()> {
        let mut words = text.split_whitespace();
        match words.next() {
            Some(".speed") => return self.set_speed(guid, words.next(), words.next().is_some()),
            Some(".vote") => {
                let reply = self.host.vote_by_name(guid, words.next());
                return self.send(packets::system_message(&reply));
            }
            _ => {}
        }
        let Some(pose) = self.map.pose(guid) else {
            return Ok(());
        };
        // One map for now: everyone stands on the spawn's map.
        let here = Mark {
            map: player::SPAWN.map,
            at: pose.position,
            facing: pose.orientation,
        };
        for line in self.survey.command(text, here).unwrap_or_default() {
            tracing::info!("survey: {line}");
            self.send(packets::system_message(&line))?;
        }
        Ok(())
    }

    /// `.speed [multiplier]`: run faster for surveying, never during a game.
    fn set_speed(&self, guid: u64, arg: Option<&str>, extra: bool) -> Result<()> {
        let multiplier = match arg.map(str::parse::<f32>) {
            None => Some(1.0),
            Some(Ok(m)) if (0.5..=10.0).contains(&m) && !extra => Some(m),
            Some(_) => None,
        };
        let reply = match multiplier {
            None => "Usage: .speed <0.5 to 10>, 1 is normal".to_owned(),
            Some(_) if self.host.in_game() => "Not during a game.".to_owned(),
            Some(m) => {
                let speed = update::BASE_SPEEDS[1] * m;
                let counter = self.map.next_counter(guid).unwrap_or(0);
                self.send(packets::force_run_speed(guid, counter, speed))?;
                self.map.set_run_speed(guid, speed);
                format!("Run speed x{m} ({speed:.1} yd/s)")
            }
        };
        self.send(packets::system_message(&reply))
    }

    fn leave_world(&mut self) {
        if let Some(me) = self.in_world.take() {
            self.host.leave(me.guid);
            self.map.leave(me.guid);
            tracing::info!("world: {} left the world", me.name);
        }
    }
}
