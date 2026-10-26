//! Logons on the local farm server: any account name logs in with the one shared password, and
//! realmd keeps each logon's session key for the world server to check, as vmangos's
//! `account.sessionkey` column does.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use benilla_srp::NormalizedString;

use crate::saves::Saves;
use crate::srp::Verifier;

/// The password every account logs in with; the launcher types it for the player.
pub const PASSWORD: &str = "FARM";

#[derive(Clone)]
pub struct Accounts {
    keys: Arc<Mutex<HashMap<String, [u8; 40]>>>,
    saves: Saves,
}

impl Accounts {
    pub fn new(saves: Saves) -> Self {
        Self {
            keys: Arc::default(),
            saves,
        }
    }

    fn keys(&self) -> std::sync::MutexGuard<'_, HashMap<String, [u8; 40]>> {
        self.keys.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A fresh salt and verifier for `user` with [`PASSWORD`].
    pub fn verifier(&self, user: &str) -> Option<Verifier> {
        let name = NormalizedString::new(user).ok()?;
        let pass = NormalizedString::new(PASSWORD).ok()?;
        Some(Verifier::new(&name, &pass))
    }

    /// `user` proved the password: keep the session key the world server checks.
    pub fn admit(&self, user: &str, _verifier: &Verifier, session_key: [u8; 40]) {
        self.keys().insert(user.to_owned(), session_key);
    }

    pub fn session_key(&self, user: &str) -> Option<[u8; 40]> {
        self.keys().get(user).copied()
    }

    /// The characters the realm list counts: every farmer in the save.
    pub fn characters(&self, _user: &str) -> Vec<farm_rules::save::Character> {
        self.saves.roster()
    }
}
