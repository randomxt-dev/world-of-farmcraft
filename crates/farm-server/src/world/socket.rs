//! World-socket framing. Server headers are 4 bytes (`u16` BE size counting the opcode, `u16` LE
//! opcode), client headers 6 (`u16` BE size, `u32` LE opcode); after `CMSG_AUTH_SESSION` both run
//! through the session-key header cipher, bodies never (`benilla_srp::vanilla_header`).

use anyhow::{Context, Result};
use benilla_srp::{DecrypterHalf, EncrypterHalf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;

/// One server packet, queued for a session's writer task.
#[derive(Debug, Clone)]
pub struct Packet {
    pub opcode: u16,
    pub body: Vec<u8>,
}

impl Packet {
    pub fn new(opcode: u16, body: Vec<u8>) -> Self {
        Self { opcode, body }
    }
}

/// A session's outbound queue; cheap to clone, so other sessions can broadcast into it.
pub type Outbox = mpsc::UnboundedSender<Packet>;

/// The 4-byte server header for `body_len` bytes of body.
fn server_header(opcode: u16, body_len: usize) -> [u8; 4] {
    let s = ((body_len + 2) as u16).to_be_bytes();
    let o = opcode.to_le_bytes();
    [s[0], s[1], o[0], o[1]]
}

/// Write one packet with a plain header (`SMSG_AUTH_CHALLENGE`, the only one).
pub async fn write_plain(w: &mut OwnedWriteHalf, packet: &Packet) -> Result<()> {
    let mut bytes = server_header(packet.opcode, packet.body.len()).to_vec();
    bytes.extend_from_slice(&packet.body);
    w.write_all(&bytes).await.context("writing a packet")
}

/// Drain `rx` onto the socket, headers encrypted, until every sender is gone or the socket dies.
pub async fn run_writer(
    mut w: OwnedWriteHalf,
    mut encrypter: EncrypterHalf,
    mut rx: mpsc::UnboundedReceiver<Packet>,
) {
    while let Some(packet) = rx.recv().await {
        let mut header = server_header(packet.opcode, packet.body.len());
        encrypter.encrypt(&mut header);
        let mut bytes = Vec::with_capacity(4 + packet.body.len());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&packet.body);
        if w.write_all(&bytes).await.is_err() {
            return;
        }
    }
}

/// Read one client packet, decrypting its header when `decrypter` is given.
pub async fn read_packet(
    r: &mut OwnedReadHalf,
    decrypter: Option<&mut DecrypterHalf>,
) -> Result<(u16, Vec<u8>)> {
    let mut header = [0u8; 6];
    r.read_exact(&mut header)
        .await
        .context("reading a header")?;
    if let Some(d) = decrypter {
        d.decrypt(&mut header);
    }
    let size = u16::from_be_bytes([header[0], header[1]]) as usize;
    let opcode = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
    let mut body = vec![0u8; size.saturating_sub(4)];
    r.read_exact(&mut body)
        .await
        .with_context(|| format!("reading the body of opcode {opcode:#06x}"))?;
    Ok((opcode as u16, body))
}
