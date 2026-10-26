//! realmd, login protocol version 3: logon challenge, logon proof, then realm lists on request
//! until the client hangs up. The packets mirror `benilla-protocol`'s `auth.rs` readers.

use std::net::SocketAddr;

use anyhow::{bail, Context, Result};
use benilla_srp::{NormalizedString, GENERATOR, LARGE_SAFE_PRIME_LITTLE_ENDIAN};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::lobbies::Lobbies;
use crate::srp::ServerChallenge;

const CMD_AUTH_LOGON_CHALLENGE: u8 = 0x00;
const CMD_AUTH_LOGON_PROOF: u8 = 0x01;
const CMD_REALM_LIST: u8 = 0x10;

/// `WOW_SUCCESS`.
const RESULT_SUCCESS: u8 = 0x00;
/// `WOW_FAIL_UNKNOWN_ACCOUNT`, vmangos's answer to an unknown account and a wrong password alike.
const RESULT_UNKNOWN_ACCOUNT: u8 = 0x04;
/// `WOW_FAIL_VERSION_INVALID`: anything but build 5875.
const RESULT_VERSION_INVALID: u8 = 0x09;

/// The `crc_salt` every mangos-family realmd sends; the client answers it with a per-build constant
/// we do not check.
const VERSION_CHALLENGE: [u8; 16] = [
    0xba, 0xa3, 0x1e, 0x99, 0xa0, 0x0b, 0x21, 0x57, 0xfc, 0x37, 0x3f, 0xb3, 0x69, 0xcd, 0xd2, 0xf1,
];

/// What realmd advertises.
#[derive(Clone)]
pub struct RealmConfig {
    pub name: String,
    /// The world server's host; `None` is the address the client reached realmd on.
    pub advertise: Option<String>,
    pub world_port: u16,
}

impl RealmConfig {
    /// The world address for a client that reached realmd on `local`: a LAN guest gets the LAN
    /// address, unless a public one is advertised for players behind the host's router.
    fn world_address(&self, local: std::net::IpAddr) -> String {
        match &self.advertise {
            Some(host) => format!("{host}:{}", self.world_port),
            None => format!("{local}:{}", self.world_port),
        }
    }
}

/// Accept logons forever, each into the lobby [`Lobbies::route`] picks for it.
pub async fn serve(listener: TcpListener, lobbies: Lobbies, realm: RealmConfig) -> Result<()> {
    loop {
        let (stream, peer) = listener.accept().await.context("accepting a logon")?;
        let lobbies = lobbies.clone();
        let realm = realm.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, peer, lobbies, realm).await {
                tracing::info!("realmd {peer}: {e:#}");
            }
        });
    }
}

async fn handle(
    mut stream: TcpStream,
    peer: SocketAddr,
    lobbies: Lobbies,
    realm: RealmConfig,
) -> Result<()> {
    stream.set_nodelay(true)?;
    let world_address = realm.world_address(stream.local_addr()?.ip());

    // ── CMD_AUTH_LOGON_CHALLENGE: cmd, protocol, u16 size, then `size` bytes ──
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await?;
    if head[0] != CMD_AUTH_LOGON_CHALLENGE {
        bail!("expected a logon challenge, got command {:#04x}", head[0]);
    }
    let size = u16::from_le_bytes([head[2], head[3]]) as usize;
    let mut body = vec![0u8; size];
    stream.read_exact(&mut body).await?;
    let challenge = parse_challenge(&body)?;
    tracing::info!(
        "realmd {peer}: logon challenge for {} (build {})",
        challenge.account,
        challenge.build
    );

    if challenge.build != benilla_protocol::CLIENT_BUILD {
        stream
            .write_all(&[CMD_AUTH_LOGON_CHALLENGE, 0, RESULT_VERSION_INVALID])
            .await?;
        bail!("build {} refused", challenge.build);
    }
    // SRP commits to one verifier here, before the password is proved: the name and the
    // connection pick the lobby, whose code is then the password.
    let Some(lobby) = lobbies.route(&challenge.account, peer) else {
        stream
            .write_all(&[CMD_AUTH_LOGON_CHALLENGE, 0, RESULT_UNKNOWN_ACCOUNT])
            .await?;
        bail!(
            "no lobby for {}: with several open, a native name ends in -CODE",
            challenge.account
        );
    };
    let accounts = lobby.accounts().clone();
    let (Ok(user), Some(verifier)) = (
        NormalizedString::new(&challenge.account),
        accounts.verifier(&challenge.account),
    ) else {
        stream
            .write_all(&[CMD_AUTH_LOGON_CHALLENGE, 0, RESULT_UNKNOWN_ACCOUNT])
            .await?;
        bail!("unknown account {}", challenge.account);
    };
    let srp = ServerChallenge::new(user, &verifier);
    stream.write_all(&challenge_reply(&srp)).await?;

    // ── CMD_AUTH_LOGON_PROOF: cmd, A[32], M1[20], crc_hash[20], key count, security flag ──
    let mut proof = [0u8; 75];
    stream.read_exact(&mut proof).await?;
    if proof[0] != CMD_AUTH_LOGON_PROOF {
        bail!("expected a logon proof, got command {:#04x}", proof[0]);
    }
    // Telemetry keys (30 bytes each) follow; benilla sends none, a stock client a few.
    let mut keys = vec![0u8; 30 * proof[73] as usize];
    stream.read_exact(&mut keys).await?;
    let a: [u8; 32] = proof[1..33].try_into().expect("32 bytes");
    let m1: [u8; 20] = proof[33..53].try_into().expect("20 bytes");
    let Some(verified) = srp.verify(&a, &m1) else {
        stream
            .write_all(&[CMD_AUTH_LOGON_PROOF, RESULT_UNKNOWN_ACCOUNT])
            .await?;
        bail!("wrong password for {}", challenge.account);
    };
    accounts.admit(&challenge.account, &verifier, verified.session_key);
    let mut reply = Vec::with_capacity(26);
    reply.extend_from_slice(&[CMD_AUTH_LOGON_PROOF, RESULT_SUCCESS]);
    reply.extend_from_slice(&verified.server_proof);
    reply.extend_from_slice(&0u32.to_le_bytes()); // hardware_survey_id
    stream.write_all(&reply).await?;
    tracing::info!(
        "realmd {peer}: {} logged on to lobby {}",
        challenge.account,
        lobby.code()
    );

    // ── CMD_REALM_LIST, as often as the client asks (the realm screen refreshes every 5 s) ──
    loop {
        let mut request = [0u8; 5];
        if stream.read_exact(&mut request).await.is_err() {
            return Ok(());
        }
        if request[0] != CMD_REALM_LIST {
            bail!(
                "expected a realm list request, got command {:#04x}",
                request[0]
            );
        }
        let characters = accounts.characters(&challenge.account).len() as u8;
        stream
            .write_all(&realm_list(&realm.name, &world_address, characters))
            .await?;
    }
}

struct Challenge {
    build: u16,
    /// Uppercased, as the client sends it.
    account: String,
}

/// The challenge body after the size field: game name, version, build, platform, OS, locale,
/// timezone, IP, then the length-prefixed account name.
fn parse_challenge(body: &[u8]) -> Result<Challenge> {
    if body.len() < 30 {
        bail!("logon challenge too short ({} bytes)", body.len());
    }
    let build = u16::from_le_bytes([body[7], body[8]]);
    let name_len = body[29] as usize;
    let Some(name) = body.get(30..30 + name_len) else {
        bail!("logon challenge account name overruns the packet");
    };
    Ok(Challenge {
        build,
        account: String::from_utf8_lossy(name).to_ascii_uppercase(),
    })
}

fn challenge_reply(srp: &ServerChallenge) -> Vec<u8> {
    let mut out = Vec::with_capacity(119);
    out.extend_from_slice(&[CMD_AUTH_LOGON_CHALLENGE, 0, RESULT_SUCCESS]);
    out.extend_from_slice(srp.public_key());
    out.extend_from_slice(&[1, GENERATOR]);
    out.push(32);
    out.extend_from_slice(&LARGE_SAFE_PRIME_LITTLE_ENDIAN);
    out.extend_from_slice(srp.salt());
    out.extend_from_slice(&VERSION_CHALLENGE);
    out.push(0); // security_flag: no PIN, no matrix
    out
}

/// `CMD_REALM_LIST_Server` with one realm.
fn realm_list(name: &str, address: &str, characters: u8) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_le_bytes()); // header padding
    body.push(1); // realm count
    body.extend_from_slice(&0u32.to_le_bytes()); // realm type: Normal
    body.push(0); // flags
    body.extend_from_slice(name.as_bytes());
    body.push(0);
    body.extend_from_slice(address.as_bytes());
    body.push(0);
    body.extend_from_slice(&0.5f32.to_le_bytes()); // population: medium
    body.push(characters);
    body.push(1); // category
    body.push(1); // realm id
    body.extend_from_slice(&0x0002u16.to_le_bytes()); // footer, as vmangos sends it

    let mut out = Vec::with_capacity(body.len() + 3);
    out.push(CMD_REALM_LIST);
    out.extend_from_slice(&(body.len() as u16).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_parses_the_clients_bytes() {
        let mut packet = Vec::new();
        benilla_protocol::auth::write_logon_challenge(&mut packet, "AMONG1", 5875).unwrap();
        let c = parse_challenge(&packet[4..]).unwrap();
        assert_eq!(c.build, 5875);
        assert_eq!(c.account, "AMONG1");
    }

    #[test]
    fn the_world_address_is_the_advertised_host_when_set() {
        let lan: std::net::IpAddr = [192, 168, 1, 20].into();
        let mut realm = RealmConfig {
            name: "Among Azeroth".into(),
            advertise: None,
            world_port: 8085,
        };
        assert_eq!(realm.world_address(lan), "192.168.1.20:8085");
        realm.advertise = Some("among.example.net".into());
        assert_eq!(realm.world_address(lan), "among.example.net:8085");
    }

    /// The client's own realm-list reader takes our packet, every field intact.
    #[test]
    fn the_realm_list_reads_back_in_the_client() {
        struct Cursor<'a>(&'a [u8]);
        impl benilla_protocol::transport::ReadExactAsync for Cursor<'_> {
            async fn read_exact_async(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
                std::io::Read::read_exact(&mut self.0, buf)
            }
        }
        let packet = realm_list("Among Azeroth", "127.0.0.1:8085", 2);
        let mut cursor = Cursor(&packet);
        let realms = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(benilla_protocol::auth::read_realm_list(&mut cursor))
            .unwrap();
        assert!(cursor.0.is_empty(), "the stream stays aligned");
        assert_eq!(realms.len(), 1);
        assert_eq!(realms[0].name, "Among Azeroth");
        assert_eq!(realms[0].address, "127.0.0.1:8085");
        assert_eq!(realms[0].characters, 2);
    }
}
