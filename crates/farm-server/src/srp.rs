//! The server half of 1.12.1 SRP6 (`N`, `g = 7`, `k = 3`), every value little endian as on the
//! wire. `benilla-srp` is the client half; the two must agree byte for byte.
//!
//! Every value is hashed at its full declared width, as `benilla-srp` does. `b` is redrawn until
//! `B`'s high byte is non-zero, so the client never has to redial for an unambiguous challenge.

use benilla_srp::{NormalizedString, GENERATOR, LARGE_SAFE_PRIME_LITTLE_ENDIAN};
use num_bigint::{BigInt, Sign};
use rand::RngCore;
use sha1::{Digest, Sha1};

/// The SRP6 multiplier `k`, 3 in 1.12 (SRP6, not 6a).
const K_VALUE: u8 = 3;

fn from_le(bytes: &[u8]) -> BigInt {
    BigInt::from_bytes_le(Sign::Plus, bytes)
}

/// `v` as 32 zero-padded little-endian bytes (`v < N < 2^256`).
fn to_le_32(v: &BigInt) -> [u8; 32] {
    let (_, bytes) = v.to_bytes_le();
    let mut out = [0u8; 32];
    out[..bytes.len()].copy_from_slice(&bytes);
    out
}

fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut h = Sha1::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// WoW's `SHA1_Interleave`: hash the even and odd bytes of `S` apart and interleave the digests.
fn interleave(s: &[u8; 32]) -> [u8; 40] {
    let even: Vec<u8> = s.iter().step_by(2).copied().collect();
    let odd: Vec<u8> = s.iter().skip(1).step_by(2).copied().collect();
    let (g, h) = (sha1(&[&even]), sha1(&[&odd]));
    let mut out = [0u8; 40];
    for i in 0..20 {
        out[i * 2] = g[i];
        out[i * 2 + 1] = h[i];
    }
    out
}

/// An account's stored SRP6 secret: the salt and the verifier `v = g^x mod N`.
#[derive(Debug, Clone)]
pub struct Verifier {
    pub salt: [u8; 32],
    pub verifier: [u8; 32],
}

impl Verifier {
    /// A fresh salt and verifier for `user`/`pass`, as an account is created.
    pub fn new(user: &NormalizedString, pass: &NormalizedString) -> Self {
        let (salt, verifier) = benilla_srp::generate_account(user, pass);
        Self { salt, verifier }
    }
}

/// One logon in flight: the challenge sent, waiting for the client's proof.
pub struct ServerChallenge {
    username: NormalizedString,
    salt: [u8; 32],
    verifier: BigInt,
    private_key: BigInt,
    public_key: [u8; 32],
}

/// A verified logon: the 40-byte session key and the server proof `M2` to send back.
pub struct ServerProof {
    pub session_key: [u8; 40],
    pub server_proof: [u8; 20],
}

impl ServerChallenge {
    /// Draw `b` and compute `B = k·v + g^b mod N`, redrawing until `B`'s high byte is non-zero.
    pub fn new(username: NormalizedString, account: &Verifier) -> Self {
        let n = from_le(&LARGE_SAFE_PRIME_LITTLE_ENDIAN);
        let g = BigInt::from(GENERATOR);
        let v = from_le(&account.verifier);
        let mut rng = rand::thread_rng();
        loop {
            let mut b = [0u8; 19];
            rng.fill_bytes(&mut b);
            let b = from_le(&b);
            let public = (BigInt::from(K_VALUE) * &v + g.modpow(&b, &n)) % &n;
            let public_key = to_le_32(&public);
            if public_key[31] != 0 {
                return Self {
                    username,
                    salt: account.salt,
                    verifier: v,
                    private_key: b,
                    public_key,
                };
            }
        }
    }

    /// `B`, for `CMD_AUTH_LOGON_CHALLENGE`.
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }

    pub fn salt(&self) -> &[u8; 32] {
        &self.salt
    }

    /// Check the client's `A` and `M1`; `None` is a wrong password (or a hostile `A`).
    pub fn verify(
        &self,
        client_public_key: &[u8; 32],
        client_proof: &[u8; 20],
    ) -> Option<ServerProof> {
        let n = from_le(&LARGE_SAFE_PRIME_LITTLE_ENDIAN);
        let a = from_le(client_public_key);
        if (&a % &n) == BigInt::from(0) {
            return None;
        }
        let u = from_le(&sha1(&[client_public_key, &self.public_key]));
        // S = (A · v^u)^b mod N
        let s = (a * self.verifier.modpow(&u, &n)).modpow(&self.private_key, &n);
        let session_key = interleave(&to_le_32(&s));

        let n_hash = sha1(&[&LARGE_SAFE_PRIME_LITTLE_ENDIAN]);
        let g_hash = sha1(&[&[GENERATOR]]);
        let mut xor = [0u8; 20];
        for i in 0..20 {
            xor[i] = n_hash[i] ^ g_hash[i];
        }
        let user_hash = sha1(&[self.username.as_ref().as_bytes()]);
        let expected = sha1(&[
            &xor,
            &user_hash,
            &self.salt,
            client_public_key,
            &self.public_key,
            &session_key,
        ]);
        if &expected != client_proof {
            return None;
        }
        let server_proof = sha1(&[client_public_key, client_proof, &session_key]);
        Some(ServerProof {
            session_key,
            server_proof,
        })
    }
}

/// The world handshake's proof: `SHA1(user | 0u32 | client_seed | server_seed | K)`.
pub fn world_proof(
    username: &str,
    client_seed: u32,
    server_seed: u32,
    session_key: &[u8; 40],
) -> [u8; 20] {
    sha1(&[
        username.as_bytes(),
        &0u32.to_le_bytes(),
        &client_seed.to_le_bytes(),
        &server_seed.to_le_bytes(),
        session_key,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_srp::{PublicKey, SrpClientChallenge};

    fn norm(s: &str) -> NormalizedString {
        NormalizedString::new(s).unwrap()
    }

    /// `benilla-srp`'s client completes a logon against this server: both sides derive one key.
    #[test]
    fn the_benilla_client_logs_on() {
        let account = Verifier::new(&norm("farmer1"), &norm("secret"));
        for _ in 0..32 {
            let server = ServerChallenge::new(norm("farmer1"), &account);
            let client = SrpClientChallenge::new(
                norm("farmer1"),
                norm("secret"),
                GENERATOR,
                LARGE_SAFE_PRIME_LITTLE_ENDIAN,
                PublicKey::from_le_bytes(*server.public_key()).unwrap(),
                *server.salt(),
            );
            let proof = server
                .verify(client.client_public_key(), client.client_proof())
                .expect("the right password verifies");
            let client = client.verify_server_proof(proof.server_proof).unwrap();
            assert_eq!(client.session_key(), &proof.session_key);
        }
    }

    #[test]
    fn a_wrong_password_is_refused() {
        let account = Verifier::new(&norm("farmer1"), &norm("secret"));
        let server = ServerChallenge::new(norm("farmer1"), &account);
        let client = SrpClientChallenge::new(
            norm("farmer1"),
            norm("wrong"),
            GENERATOR,
            LARGE_SAFE_PRIME_LITTLE_ENDIAN,
            PublicKey::from_le_bytes(*server.public_key()).unwrap(),
            *server.salt(),
        );
        assert!(server
            .verify(client.client_public_key(), client.client_proof())
            .is_none());
    }

    #[test]
    fn the_challenge_key_is_width_stable() {
        let account = Verifier::new(&norm("farmer1"), &norm("secret"));
        for _ in 0..256 {
            assert_ne!(
                ServerChallenge::new(norm("farmer1"), &account).public_key()[31],
                0
            );
        }
    }

    /// The world proof matches the client's: its header cipher derives from the same proof inputs.
    #[test]
    fn world_proof_matches_the_client() {
        let key = [9u8; 40];
        let seed = benilla_srp::ProofSeed::new();
        let client_seed = seed.seed();
        let (proof, _) = seed.into_client_header_crypto(&norm("farmer1"), key, 0x1234_5678);
        assert_eq!(
            world_proof("FARMER1", client_seed, 0x1234_5678, &key),
            proof
        );
    }
}
