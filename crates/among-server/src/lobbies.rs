//! Many lobbies in one server, keyed by their code: each holds its own accounts, map, match and
//! clock, so players in different lobbies never see each other. SRP makes realmd commit to one
//! verifier before the password is proved, so the code alone cannot pick a lobby for a new name:
//! [`Lobbies::route`] picks it from the account name and the connection instead.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::accounts::{self, Accounts};
use crate::game::GameHost;
use crate::look::Look;
use crate::world::Map;

/// How often a lobby's game clock advances: kill and sabotage cooldowns, meeting timers, deadlines.
const TICK: Duration = Duration::from_millis(100);

/// What every lobby is made from: the server's rules and map.
#[derive(Clone)]
pub struct Template {
    pub settings: among_rules::Settings,
    pub layout: among_rules::Layout,
    pub look: Look,
    pub start_countdown: Duration,
}

/// One lobby; cheap to clone. Its clock stops when the last clone is dropped.
#[derive(Clone)]
pub struct Lobby(Arc<LobbyInner>);

struct LobbyInner {
    code: String,
    accounts: Accounts,
    map: Map,
    host: GameHost,
    /// Guests log in with the code; an `--account`-only server's fixed lobby takes none.
    open: bool,
    /// Made by a player: it closes once empty for the idle time.
    created: bool,
    presence: Mutex<Presence>,
    clock: JoinHandle<()>,
}

struct Presence {
    sessions: usize,
    empty_since: Instant,
}

impl Drop for LobbyInner {
    fn drop(&mut self) {
        self.clock.abort();
    }
}

impl Lobby {
    fn start(
        code: String,
        accounts: Accounts,
        template: &Template,
        open: bool,
        created: bool,
    ) -> Self {
        let map = Map::default();
        let host = GameHost::new(
            template.settings.clone(),
            template.layout.clone(),
            template.look.clone(),
            map.clone(),
        );
        host.set_start_countdown(template.start_countdown);
        let clock = host.clone();
        let clock = tokio::spawn(async move {
            let mut every = tokio::time::interval(TICK);
            let mut last = Instant::now();
            loop {
                let now = every.tick().await;
                clock.tick(now - last);
                last = now;
            }
        });
        Self(Arc::new(LobbyInner {
            code,
            accounts,
            map,
            host,
            open,
            created,
            presence: Mutex::new(Presence {
                sessions: 0,
                empty_since: Instant::now(),
            }),
            clock,
        }))
    }

    pub fn code(&self) -> &str {
        &self.0.code
    }

    pub fn accounts(&self) -> &Accounts {
        &self.0.accounts
    }

    pub fn map(&self) -> &Map {
        &self.0.map
    }

    pub fn host(&self) -> &GameHost {
        &self.0.host
    }

    /// A world session is in the lobby until the returned guard drops.
    pub fn enter(&self) -> Present {
        self.presence().sessions += 1;
        Present(self.clone())
    }

    /// World sessions in the lobby now.
    pub fn sessions(&self) -> usize {
        self.presence().sessions
    }

    fn presence(&self) -> std::sync::MutexGuard<'_, Presence> {
        self.0
            .presence
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// A page of this lobby is active (it fetched game data): an empty lobby's idle time restarts.
    pub fn touch(&self) {
        let mut presence = self.presence();
        if presence.sessions == 0 {
            presence.empty_since = Instant::now();
        }
    }

    /// Whether a created lobby has stood empty for `idle` by `now`.
    fn expired(&self, now: Instant, idle: Duration) -> bool {
        let presence = self.presence();
        self.0.created && presence.sessions == 0 && now >= presence.empty_since + idle
    }
}

/// One world session's place in a lobby.
pub struct Present(Lobby);

impl Drop for Present {
    fn drop(&mut self) {
        let mut presence = self.0.presence();
        presence.sessions = presence.sessions.saturating_sub(1);
        if presence.sessions == 0 {
            presence.empty_since = Instant::now();
        }
    }
}

/// Lobby creation refused: the server already runs its cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full(pub usize);

impl std::fmt::Display for Full {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "All {} lobbies on this server are in use; try again in a few minutes.",
            self.0
        )
    }
}

impl std::error::Error for Full {}

struct Registry {
    template: Template,
    lobbies: HashMap<String, Lobby>,
    max: usize,
    idle: Duration,
    /// Browser relay connections by the address the server sees them from, with the lobby code
    /// their page carried: a browser logon's lobby.
    hints: HashMap<SocketAddr, String>,
}

/// The live lobbies; cheap to clone, shared by realmd, the world server and the web host.
#[derive(Clone)]
pub struct Lobbies(Arc<Mutex<Registry>>);

impl Lobbies {
    /// No lobbies yet; at most `max` live at once, a created one closing after `idle` empty.
    pub fn new(template: Template, max: usize, idle: Duration) -> Self {
        Self(Arc::new(Mutex::new(Registry {
            template,
            lobbies: HashMap::new(),
            max,
            idle,
            hints: HashMap::new(),
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The operator's lobby, which never closes: open to guests when `accounts` has a lobby
    /// code, else only its `--account`s log in. Returns its code.
    pub fn add_fixed(&self, accounts: Accounts) -> String {
        let mut registry = self.lock();
        let open = accounts.lobby_code();
        let code = open
            .clone()
            .unwrap_or_else(|| fresh_code(&registry.lobbies));
        let lobby = Lobby::start(
            code.clone(),
            accounts,
            &registry.template,
            open.is_some(),
            false,
        );
        registry.lobbies.insert(code.clone(), lobby);
        code
    }

    /// A new lobby with a fresh code, unless the cap is reached.
    pub fn create(&self) -> Result<String, Full> {
        let mut registry = self.lock();
        if registry.lobbies.len() >= registry.max {
            return Err(Full(registry.max));
        }
        let code = fresh_code(&registry.lobbies);
        let accounts = Accounts::created_lobby(&code).expect("a fresh code is a valid password");
        let lobby = Lobby::start(code.clone(), accounts, &registry.template, true, true);
        registry.lobbies.insert(code.clone(), lobby);
        tracing::info!("lobbies: {code} created ({} live)", registry.lobbies.len());
        Ok(code)
    }

    /// The live lobby guests reach with `code` (any case).
    pub fn open(&self, code: &str) -> Option<Lobby> {
        let code = code.to_ascii_uppercase();
        self.lock().lobbies.get(&code).filter(|l| l.0.open).cloned()
    }

    /// Live lobbies.
    pub fn len(&self) -> usize {
        self.lock().lobbies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A browser relay connection the server will see from `peer` belongs to lobby `code`.
    pub fn hint(&self, peer: SocketAddr, code: &str) {
        self.lock()
            .hints
            .insert(canonical(peer), code.to_ascii_uppercase());
    }

    /// The relay connection from `peer` closed.
    pub fn forget_hint(&self, peer: SocketAddr) {
        self.lock().hints.remove(&canonical(peer));
    }

    /// The lobby a logon of `user` (uppercased) from `peer` is for, in order: a `NAME-CODE` name
    /// naming an open lobby, the page code a browser relay carried, the one lobby that already
    /// knows the name, the only open lobby. `None` when nothing settles it.
    pub fn route(&self, user: &str, peer: SocketAddr) -> Option<Lobby> {
        let registry = self.lock();
        let open = |code: &str| registry.lobbies.get(code).filter(|l| l.0.open);
        if let Some(lobby) = user.rsplit_once('-').and_then(|(_, code)| open(code)) {
            return Some(lobby.clone());
        }
        if let Some(lobby) = registry.hints.get(&canonical(peer)).and_then(|c| open(c)) {
            return Some(lobby.clone());
        }
        let mut knowing = registry
            .lobbies
            .values()
            .filter(|l| l.0.accounts.knows(user));
        if let (Some(lobby), None) = (knowing.next(), knowing.next()) {
            return Some(lobby.clone());
        }
        let mut open = registry.lobbies.values().filter(|l| l.0.open);
        match (open.next(), open.next()) {
            (Some(lobby), None) => Some(lobby.clone()),
            _ => None,
        }
    }

    /// The lobby whose realmd logon of `account` gave the session key `proves` accepts, and
    /// that key: a name may be logged on in several lobbies, each with its own key.
    pub fn logged_on(
        &self,
        account: &str,
        mut proves: impl FnMut(&[u8; 40]) -> bool,
    ) -> Option<(Lobby, [u8; 40])> {
        let lobbies: Vec<Lobby> = self.lock().lobbies.values().cloned().collect();
        lobbies.into_iter().find_map(|lobby| {
            let key = lobby.accounts().session_key(account)?;
            proves(&key).then_some((lobby, key))
        })
    }

    /// Close every created lobby that has stood empty for the idle time by `now`; returns their
    /// codes.
    pub fn sweep(&self, now: Instant) -> Vec<String> {
        let mut registry = self.lock();
        let idle = registry.idle;
        let mut closed = Vec::new();
        registry.lobbies.retain(|code, lobby| {
            let expired = lobby.expired(now, idle);
            if expired {
                closed.push(code.clone());
            }
            !expired
        });
        let live = registry.lobbies.len();
        for code in &closed {
            tracing::info!("lobbies: {code} closed, empty ({live} live)");
        }
        let Registry { hints, lobbies, .. } = &mut *registry;
        hints.retain(|_, code| lobbies.contains_key(code));
        closed
    }
}

/// A code no live lobby has.
fn fresh_code(lobbies: &HashMap<String, Lobby>) -> String {
    loop {
        let code = accounts::new_lobby_code();
        if !lobbies.contains_key(&code) {
            return code;
        }
    }
}

/// `peer` as an IPv4 address when it is one mapped into IPv6, as a dual-stack listener reports
/// an IPv4 client.
fn canonical(peer: SocketAddr) -> SocketAddr {
    SocketAddr::new(peer.ip().to_canonical(), peer.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template() -> Template {
        Template {
            settings: among_rules::Settings::default(),
            layout: crate::game::placeholder_layout(),
            look: crate::game::placeholder_look(),
            start_countdown: Duration::ZERO,
        }
    }

    fn lobbies(max: usize) -> Lobbies {
        Lobbies::new(template(), max, Duration::from_secs(600))
    }

    fn peer(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    /// `user` logs on to `lobby` with its code, as realmd's proof does.
    fn admit(lobby: &Lobby, user: &str) {
        let verifier = lobby.accounts().verifier(user).unwrap();
        lobby.accounts().admit(user, &verifier, [9; 40]);
    }

    #[tokio::test]
    async fn the_only_open_lobby_takes_any_new_name() {
        let lobbies = lobbies(8);
        let code = lobbies.add_fixed(Accounts::open_lobby("K3XQ9P").unwrap());
        assert_eq!(code, "K3XQ9P");
        assert_eq!(lobbies.route("THRALL", peer(1)).unwrap().code(), "K3XQ9P");
    }

    #[tokio::test]
    async fn with_two_lobbies_the_name_suffix_the_relay_hint_or_a_known_name_routes() {
        let lobbies = lobbies(8);
        lobbies.add_fixed(Accounts::open_lobby("K3XQ9P").unwrap());
        let other = lobbies.create().unwrap();
        assert!(lobbies.route("THRALL", peer(1)).is_none(), "ambiguous");

        let suffixed = format!("THRALL-{other}");
        assert_eq!(lobbies.route(&suffixed, peer(1)).unwrap().code(), other);

        lobbies.hint(peer(2), &other.to_ascii_lowercase());
        assert_eq!(lobbies.route("JAINA", peer(2)).unwrap().code(), other);
        lobbies.forget_hint(peer(2));
        assert!(lobbies.route("JAINA", peer(2)).is_none());

        admit(&lobbies.open("k3xq9p").unwrap(), "JAINA");
        assert_eq!(lobbies.route("JAINA", peer(3)).unwrap().code(), "K3XQ9P");
    }

    #[tokio::test]
    async fn a_dual_stack_peer_matches_its_ipv4_hint() {
        let lobbies = lobbies(8);
        lobbies.add_fixed(Accounts::open_lobby("K3XQ9P").unwrap());
        let other = lobbies.create().unwrap();
        lobbies.hint(peer(7), &other);
        let mapped = SocketAddr::new(std::net::Ipv4Addr::LOCALHOST.to_ipv6_mapped().into(), 7);
        assert_eq!(lobbies.route("THRALL", mapped).unwrap().code(), other);
    }

    #[tokio::test]
    async fn two_lobbies_keep_their_own_accounts_maps_and_matches() {
        let lobbies = lobbies(8);
        let a = lobbies.open(&lobbies.create().unwrap()).unwrap();
        let b = lobbies.open(&lobbies.create().unwrap()).unwrap();
        admit(&a, "THRALL");
        assert!(a.accounts().knows("THRALL"));
        assert!(!b.accounts().knows("THRALL"));
        // The same name logs on in each with that lobby's own code and key.
        let in_b = b.accounts().verifier("THRALL").unwrap();
        b.accounts().admit("THRALL", &in_b, [4; 40]);
        let (found, key) = lobbies.logged_on("THRALL", |k| *k == [4; 40]).unwrap();
        assert_eq!((found.code(), key), (b.code(), [4; 40]));
        // A character entering one map is no occupant of the other.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let thrall = a.accounts().characters("THRALL")[0].clone();
        a.map()
            .enter(thrall, tx, crate::world::player::spawn_living());
        assert_eq!((a.map().population(), b.map().population()), (1, 0));
    }

    #[tokio::test]
    async fn the_cap_refuses_creation_with_a_clear_message() {
        let lobbies = lobbies(2);
        lobbies.add_fixed(Accounts::open_lobby("K3XQ9P").unwrap());
        lobbies.create().unwrap();
        let full = lobbies.create().unwrap_err();
        assert_eq!(full, Full(2));
        assert!(full.to_string().contains("All 2 lobbies"), "{full}");
    }

    #[tokio::test]
    async fn a_page_still_loading_keeps_an_empty_lobby_open() {
        let lobbies = lobbies(2);
        let code = lobbies.create().unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let from_creation = Instant::now() + Duration::from_secs(600);
        lobbies.open(&code).unwrap().touch();
        assert!(lobbies.sweep(from_creation).is_empty());
        assert_eq!(
            lobbies.sweep(Instant::now() + Duration::from_secs(601)),
            vec![code]
        );
    }

    #[tokio::test]
    async fn an_empty_created_lobby_closes_after_the_idle_time_and_frees_its_slot() {
        let lobbies = lobbies(2);
        let fixed = lobbies.add_fixed(Accounts::open_lobby("K3XQ9P").unwrap());
        let code = lobbies.create().unwrap();
        let soon = Instant::now() + Duration::from_secs(60);
        let later = Instant::now() + Duration::from_secs(601);
        assert!(lobbies.sweep(soon).is_empty());

        // A session inside keeps it open however long.
        let present = lobbies.open(&code).unwrap().enter();
        assert!(lobbies.sweep(later).is_empty());
        drop(present);
        assert!(lobbies
            .sweep(Instant::now() + Duration::from_secs(60))
            .is_empty());

        assert_eq!(
            lobbies.sweep(Instant::now() + Duration::from_secs(601)),
            vec![code.clone()]
        );
        assert!(lobbies.open(&code).is_none());
        assert!(
            lobbies.open(&fixed).is_some(),
            "the operator's lobby never closes"
        );
        lobbies
            .create()
            .expect("the closed lobby's slot is free again");
    }

    #[tokio::test]
    async fn an_account_only_lobby_takes_no_guests_by_code() {
        let lobbies = lobbies(8);
        let accounts = Accounts::default();
        accounts.register("among1", "secret1").unwrap();
        let code = lobbies.add_fixed(accounts);
        assert!(lobbies.open(&code).is_none());
        assert_eq!(lobbies.route("AMONG1", peer(1)).unwrap().code(), code);
        assert!(lobbies.route("THRALL", peer(1)).is_none());
    }
}
