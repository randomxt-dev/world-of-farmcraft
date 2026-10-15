//! Accounts, their session keys and their characters, held in memory for the server's lifetime.
//! realmd stores the session key a logon derives; the world server reads it back to verify
//! `CMSG_AUTH_SESSION`, as vmangos's shared `account.sessionkey` column does.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use benilla_srp::NormalizedString;

use crate::srp::Verifier;

/// One playable character as the roster and the world know it.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterRecord {
    pub guid: u64,
    pub name: String,
    pub race: u8,
    pub class: u8,
    pub gender: u8,
    pub skin: u8,
    pub face: u8,
    pub hair_style: u8,
    pub hair_color: u8,
    pub facial_hair: u8,
    pub level: u8,
}

struct Account {
    verifier: Verifier,
    session_key: Option<[u8; 40]>,
    characters: Vec<CharacterRecord>,
    /// Made by the operator (`--account`), not by a guest's lobby code.
    operator: bool,
}

#[derive(Default)]
struct Inner {
    accounts: HashMap<String, Account>,
    next_guid: u64,
    /// An open lobby's shared password: any unknown name logs in with it and is registered.
    lobby_code: Option<NormalizedString>,
}

/// The account table, shared by realmd and the world server.
#[derive(Clone, Default)]
pub struct Accounts(Arc<Mutex<Inner>>);

/// Characters an account may hold, the 1.12 roster's ten rows.
pub const MAX_CHARACTERS: usize = 10;

impl Accounts {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Register `user` with `pass` (both normalised: ASCII, uppercased, 1..=16 bytes).
    pub fn register(&self, user: &str, pass: &str) -> anyhow::Result<()> {
        let user = NormalizedString::new(user).map_err(|e| anyhow::anyhow!("account name: {e}"))?;
        let pass = NormalizedString::new(pass).map_err(|e| anyhow::anyhow!("password: {e}"))?;
        let verifier = Verifier::new(&user, &pass);
        self.lock().accounts.insert(
            user.as_ref().to_owned(),
            Account {
                verifier,
                session_key: None,
                characters: Vec::new(),
                operator: true,
            },
        );
        Ok(())
    }

    /// Whether `user` may survey: an operator's account when there are any, else whoever holds
    /// the lobby (`lobby_host`), so a guest who inherits the host seat cannot rewrite the marks.
    pub fn may_survey(&self, user: &str, lobby_host: bool) -> bool {
        let inner = self.lock();
        if inner.accounts.values().any(|a| a.operator) {
            inner.accounts.get(user).is_some_and(|a| a.operator)
        } else {
            lobby_host
        }
    }

    /// An open lobby: any account name logs in with `code` as its password.
    pub fn open_lobby(code: &str) -> anyhow::Result<Self> {
        let code = NormalizedString::new(code).map_err(|e| anyhow::anyhow!("lobby code: {e}"))?;
        let accounts = Self::default();
        accounts.lock().lobby_code = Some(code);
        Ok(accounts)
    }

    /// The account's salt and verifier; in an open lobby an unknown name gets a fresh one made
    /// from the lobby code, kept only once its proof succeeds ([`Accounts::admit`]), and `None`
    /// otherwise.
    pub fn verifier(&self, user: &str) -> Option<Verifier> {
        let inner = self.lock();
        if let Some(a) = inner.accounts.get(user) {
            return Some(a.verifier.clone());
        }
        let code = inner.lobby_code.as_ref()?;
        let name = NormalizedString::new(user).ok()?;
        Some(Verifier::new(&name, code))
    }

    /// A logon proved `verifier` for `user`: keep the account (a lobby newcomer's is made now, so
    /// a name tried with a wrong code leaves nothing behind) and its session key.
    pub fn admit(&self, user: &str, verifier: &Verifier, session_key: [u8; 40]) {
        let mut inner = self.lock();
        let account = inner
            .accounts
            .entry(user.to_owned())
            .or_insert_with(|| Account {
                verifier: verifier.clone(),
                session_key: None,
                characters: Vec::new(),
                operator: false,
            });
        account.session_key = Some(session_key);
    }

    pub fn session_key(&self, user: &str) -> Option<[u8; 40]> {
        self.lock().accounts.get(user).and_then(|a| a.session_key)
    }

    /// The account's characters; an account with none gets one named after it, so a scripted
    /// `WOW_CHAR` finds a character on a fresh server.
    pub fn characters(&self, user: &str) -> Vec<CharacterRecord> {
        let mut inner = self.lock();
        let Inner {
            accounts,
            next_guid,
            ..
        } = &mut *inner;
        let Some(account) = accounts.get_mut(user) else {
            return Vec::new();
        };
        if account.characters.is_empty() {
            *next_guid += 1;
            account.characters.push(CharacterRecord {
                guid: *next_guid,
                name: default_name(user),
                race: 1,
                class: random_human_class(),
                gender: 0,
                skin: 0,
                face: 0,
                hair_style: 0,
                hair_color: 0,
                facial_hair: 0,
                level: 1,
            });
        }
        account.characters.clone()
    }

    /// Add a character; `Err` carries the `SMSG_CHAR_CREATE` result code.
    pub fn create_character(&self, user: &str, mut record: CharacterRecord) -> Result<(), u8> {
        let mut inner = self.lock();
        let taken = inner
            .accounts
            .values()
            .flat_map(|a| &a.characters)
            .any(|c| c.name.eq_ignore_ascii_case(&record.name));
        if taken {
            return Err(benilla_protocol::messages::CHAR_CREATE_NAME_IN_USE);
        }
        inner.next_guid += 1;
        record.guid = inner.next_guid;
        let Some(account) = inner.accounts.get_mut(user) else {
            return Err(CHAR_CREATE_ERROR);
        };
        if account.characters.len() >= MAX_CHARACTERS {
            return Err(benilla_protocol::messages::CHAR_CREATE_SERVER_LIMIT);
        }
        account.characters.push(record);
        Ok(())
    }

    /// Remove the account's character `guid`; whether it was there.
    pub fn delete_character(&self, user: &str, guid: u64) -> bool {
        let mut inner = self.lock();
        let Some(account) = inner.accounts.get_mut(user) else {
            return false;
        };
        let before = account.characters.len();
        account.characters.retain(|c| c.guid != guid);
        account.characters.len() != before
    }

    pub fn character(&self, user: &str, guid: u64) -> Option<CharacterRecord> {
        self.characters(user).into_iter().find(|c| c.guid == guid)
    }
}

/// The classes a 1.12 human may be (`CharBaseInfo.dbc`): warrior, paladin, rogue, priest, mage,
/// warlock. The auto-made character draws one, so its starting gear exists.
const HUMAN_CLASSES: [u8; 6] = [1, 2, 4, 5, 8, 9];

fn random_human_class() -> u8 {
    use rand::seq::SliceRandom;
    *HUMAN_CLASSES
        .choose(&mut rand::thread_rng())
        .expect("non-empty")
}

/// A fresh lobby code: six letters and digits with the look-alikes (0/O, 1/I/L) left out, so it
/// reads aloud and types cleanly as a password.
pub fn new_lobby_code() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..6)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

/// `CHAR_CREATE_ERROR` (0x2F, vmangos `SharedDefines.h`), the generic create failure.
pub const CHAR_CREATE_ERROR: u8 = 0x2F;

/// `AMONG1` → `Among1`: the roster's capitalised form of an account name.
fn default_name(user: &str) -> String {
    let lower = user.to_ascii_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_account_gets_one_character_named_after_it() {
        let accounts = Accounts::default();
        accounts.register("among1", "secret").unwrap();
        let roster = accounts.characters("AMONG1");
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].name, "Among1");
        // Stable across calls: the default is made once.
        assert_eq!(accounts.characters("AMONG1"), roster);
    }

    #[test]
    fn an_open_lobby_keeps_a_name_only_once_its_logon_is_proved() {
        let accounts = Accounts::open_lobby("k3xq9p").unwrap();
        let joining = accounts
            .verifier("THRALL")
            .expect("an unknown name may try");
        assert!(
            accounts.characters("THRALL").is_empty(),
            "nothing kept before the proof"
        );
        accounts.admit("THRALL", &joining, [7; 40]);
        // The same verifier on a second logon: the account was kept, not remade.
        assert_eq!(accounts.verifier("THRALL").unwrap().salt, joining.salt);
        assert_eq!(accounts.session_key("THRALL"), Some([7; 40]));
        assert_eq!(accounts.characters("THRALL")[0].name, "Thrall");
    }

    #[test]
    fn operators_survey_when_there_are_any_else_the_lobby_host() {
        let open = Accounts::open_lobby("k3xq9p").unwrap();
        let guest = open.verifier("GUEST").unwrap();
        open.admit("GUEST", &guest, [1; 40]);
        assert!(
            open.may_survey("GUEST", true),
            "no operators: the lobby host"
        );
        assert!(!open.may_survey("GUEST", false));
        open.register("owner", "secret").unwrap();
        assert!(
            !open.may_survey("GUEST", true),
            "a guest in the host seat may not"
        );
        assert!(open.may_survey("OWNER", false));
    }

    #[test]
    fn a_closed_server_refuses_unknown_names() {
        let accounts = Accounts::default();
        accounts.register("among1", "secret").unwrap();
        assert!(accounts.verifier("AMONG1").is_some());
        assert!(accounts.verifier("THRALL").is_none());
    }

    #[test]
    fn lobby_codes_are_six_unambiguous_characters() {
        let code = new_lobby_code();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| !"01OIL".contains(c)), "{code}");
        assert!(NormalizedString::new(&code).is_ok());
    }

    #[test]
    fn names_are_unique_across_accounts() {
        let accounts = Accounts::default();
        accounts.register("a", "p").unwrap();
        accounts.register("b", "p").unwrap();
        let mut rec = accounts.characters("A")[0].clone();
        rec.name = "Thrall".into();
        assert_eq!(accounts.create_character("A", rec.clone()), Ok(()));
        assert_eq!(
            accounts.create_character("B", rec),
            Err(benilla_protocol::messages::CHAR_CREATE_NAME_IN_USE)
        );
    }
}
