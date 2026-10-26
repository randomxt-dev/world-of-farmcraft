//! One world connection: the auth handshake, the character roster, then the farmer's game until
//! the socket closes. A reader task turns the socket into a stream of packets, so the session
//! loop can wait on them and on the game's tick together.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use benilla_protocol::messages::{self, opcode};
use benilla_protocol::wire::{read_cstring, read_u32_le, read_u64_le, read_u8};
use benilla_srp::vanilla_header::HeaderCrypto;
use farm_rules::save::Character;
use rand::RngCore;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use super::packets;
use super::player;
use super::socket::{self, Outbox, Packet};
use super::spells::CastRequest;
use crate::accounts::Accounts;
use crate::game::Game;
use crate::layout::Layout;
use crate::saves::{Saves, CREATE_ERROR};
use crate::srp::world_proof;

/// `CHAR_DELETE_FAILED` (vmangos `SharedDefines.h`).
const CHAR_DELETE_FAILED: u8 = 0x3A;
/// How often the game runs between packets.
const TICK: Duration = Duration::from_millis(100);

pub async fn run(
    stream: TcpStream,
    accounts: Accounts,
    saves: Saves,
    layout: Arc<Layout>,
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
    let key = accounts
        .session_key(&account)
        .filter(|key| world_proof(&account, client_seed, server_seed, key) == digest);
    let Some(key) = key else {
        socket::write_plain(&mut w, &packets::auth_refused(messages::AUTH_FAILED)).await?;
        bail!("{account}: no realmd logon proves this session");
    };

    let (encrypter, mut decrypter) = HeaderCrypto::from_session_key(key).split();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(socket::run_writer(w, encrypter, rx));
    tx.send(packets::auth_ok())?;
    tracing::info!("world: {account} authenticated");

    let (in_tx, mut inbox) = mpsc::unbounded_channel();
    let reader = tokio::spawn(async move {
        loop {
            let packet = socket::read_packet(&mut r, Some(&mut decrypter)).await;
            let end = packet.is_err();
            if in_tx.send(packet).is_err() || end {
                return;
            }
        }
    });

    let mut session = Session {
        account,
        saves,
        layout,
        outbox: tx,
        game: None,
    };
    let mut tick = tokio::time::interval(TICK);
    let mut last = tokio::time::Instant::now();
    let result = loop {
        tokio::select! {
            packet = inbox.recv() => match packet {
                Some(Ok((op, body))) => {
                    if let Err(e) = session.handle(op, &body) {
                        break Err(e);
                    }
                }
                Some(Err(e)) => break Err(e),
                None => break Ok(()),
            },
            now = tick.tick() => {
                if let Some(game) = &mut session.game {
                    game.tick(now - last);
                }
                last = now;
            }
        }
    };
    reader.abort();
    session.leave_world();
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
    saves: Saves,
    layout: Arc<Layout>,
    outbox: Outbox,
    /// The farmer's game, once `CMSG_PLAYER_LOGIN` seated them.
    game: Option<Game>,
}

impl Session {
    fn send(&self, packet: Packet) -> Result<()> {
        self.outbox.send(packet).context("the writer is gone")
    }

    fn handle(&mut self, op: u16, body: &[u8]) -> Result<()> {
        let mut cur = body;
        match op {
            opcode::CMSG_CHAR_ENUM => {
                let roster = self.saves.roster();
                self.send(packets::char_enum(&roster, self.layout.home))?;
            }
            opcode::CMSG_CHAR_CREATE => {
                let name = read_cstring(&mut cur)?;
                let b = benilla_protocol::wire::read_array::<9>(&mut cur)?;
                let character = Character {
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
                };
                let code = if character.name.is_empty() {
                    CREATE_ERROR
                } else {
                    match self.saves.create(character) {
                        Ok(()) => messages::CHAR_CREATE_SUCCESS,
                        Err(code) => code,
                    }
                };
                self.send(packets::char_create_result(code))?;
            }
            opcode::CMSG_CHAR_DELETE => {
                let guid = read_u64_le(&mut cur)?;
                let code = if self.saves.delete(guid) {
                    messages::CHAR_DELETE_SUCCESS
                } else {
                    CHAR_DELETE_FAILED
                };
                self.send(packets::char_delete_result(code))?;
            }
            opcode::CMSG_PLAYER_LOGIN => {
                let guid = read_u64_le(&mut cur)?;
                let (Some(me), Some(farm)) = (self.saves.character(guid), self.saves.farm(guid))
                else {
                    // `CHAR_LOGIN_NO_CHARACTER`.
                    self.send(Packet::new(opcode::SMSG_CHARACTER_LOGIN_FAILED, vec![0x0D]))?;
                    return Ok(());
                };
                self.leave_world();
                tracing::info!("world: {} enters as {}", self.account, me.name);
                let mut game = Game::new(
                    me,
                    farm,
                    self.layout.clone(),
                    self.saves.clone(),
                    self.outbox.clone(),
                );
                game.enter();
                self.game = Some(game);
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
                if let Some(c) = self.saves.character(guid) {
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
                if let (Some(game), Some((at, facing))) = (&mut self.game, player::read_pose(body))
                {
                    game.moved(at, facing);
                }
            }
            opcode::CMSG_CAST_SPELL => {
                if let Some(game) = &mut self.game {
                    game.cast(CastRequest::read(body)?);
                }
            }
            opcode::CMSG_CANCEL_CAST => {
                if let Some(game) = &mut self.game {
                    game.interrupt();
                }
            }
            opcode::CMSG_ITEM_QUERY_SINGLE => {
                let entry = read_u32_le(&mut cur)?;
                self.send(super::items::query_response(entry))?;
            }
            opcode::CMSG_CREATURE_QUERY => {
                let entry = read_u32_le(&mut cur)?;
                if let Some(game) = &self.game {
                    self.send(game.creature_query(entry))?;
                }
            }
            opcode::CMSG_GAMEOBJECT_QUERY => {
                let entry = read_u32_le(&mut cur)?;
                if let Some(game) = &self.game {
                    self.send(game.gameobject_query(entry))?;
                }
            }
            opcode::CMSG_GAMEOBJ_USE => {
                let object = read_u64_le(&mut cur)?;
                if let Some(game) = &mut self.game {
                    game.use_object(object);
                }
            }
            opcode::CMSG_GOSSIP_HELLO => {
                let npc = read_u64_le(&mut cur)?;
                if let Some(game) = &mut self.game {
                    game.gossip_hello(npc);
                }
            }
            opcode::CMSG_GOSSIP_SELECT_OPTION => {
                let _speaker = read_u64_le(&mut cur)?;
                let option = read_u32_le(&mut cur)?;
                if let Some(game) = &mut self.game {
                    game.gossip_select(option);
                }
            }
            opcode::CMSG_NPC_TEXT_QUERY => {
                let id = read_u32_le(&mut cur)?;
                if let Some(game) = &self.game {
                    game.npc_text(id);
                }
            }
            opcode::CMSG_LIST_INVENTORY => {
                let vendor = read_u64_le(&mut cur)?;
                if let Some(game) = &self.game {
                    game.list_inventory(vendor);
                }
            }
            opcode::CMSG_BUY_ITEM => {
                let vendor = read_u64_le(&mut cur)?;
                let entry = read_u32_le(&mut cur)?;
                let count = read_u8(&mut cur)?;
                if let Some(game) = &mut self.game {
                    game.buy(vendor, entry, count);
                }
            }
            opcode::CMSG_SELL_ITEM => {
                let vendor = read_u64_le(&mut cur)?;
                let item = read_u64_le(&mut cur)?;
                let count = read_u8(&mut cur)?;
                if let Some(game) = &mut self.game {
                    game.sell(vendor, item, count);
                }
            }
            opcode::CMSG_DESTROYITEM => {
                let _bag = read_u8(&mut cur)?;
                let slot = read_u8(&mut cur)?;
                if let Some(game) = &mut self.game {
                    game.destroy_item(slot);
                }
            }
            opcode::CMSG_MESSAGECHAT => {
                let chat = packets::ClientChat::read(body)?;
                if let Some(game) = &mut self.game {
                    if chat.text.starts_with('.') {
                        game.command(&chat.text);
                    }
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

    fn leave_world(&mut self) {
        if let Some(mut game) = self.game.take() {
            game.leave();
            tracing::info!("world: {} left the world", self.account);
        }
    }
}
