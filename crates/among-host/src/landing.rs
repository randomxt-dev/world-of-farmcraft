//! The landing page anyone may open, and `POST /lobby/new`, which makes a lobby and sends its
//! maker to the lobby's invite link. Creation is limited per address and by the server's cap.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use among_server::Lobbies;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;

/// Lobbies one address may make within [`WINDOW`].
const PER_WINDOW: usize = 3;
const WINDOW: Duration = Duration::from_secs(10 * 60);

/// Lobby creations per client address, over the last [`WINDOW`].
#[derive(Clone, Default)]
pub struct Limiter(Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>>);

impl Limiter {
    /// Count one creation by `ip` at `now`, unless it has made [`PER_WINDOW`] already.
    fn allow(&self, ip: IpAddr, now: Instant) -> bool {
        let mut seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        seen.retain(|_, times| {
            times.retain(|t| now.duration_since(*t) < WINDOW);
            !times.is_empty()
        });
        let times = seen.entry(ip).or_default();
        if times.len() >= PER_WINDOW {
            return false;
        }
        times.push(now);
        true
    }
}

#[derive(Clone)]
struct PageState {
    lobbies: Lobbies,
    limiter: Limiter,
}

/// `GET /lobby` (the landing page) and `POST /lobby/new`; both open to anyone.
pub fn router(lobbies: Lobbies, limiter: Limiter) -> Router {
    Router::new()
        .route("/lobby", get(|| async { page(None) }))
        .route("/lobby/new", post(create))
        .with_state(PageState { lobbies, limiter })
}

async fn create(State(state): State<PageState>, request: Request) -> Response {
    if !state.limiter.allow(client_ip(&request), Instant::now()) {
        let why = "You made several lobbies just now; try again in a few minutes.";
        return (StatusCode::TOO_MANY_REQUESTS, page(Some(why))).into_response();
    }
    match state.lobbies.create() {
        Ok(code) => Redirect::to(&format!("/?lobby={code}")).into_response(),
        Err(full) => (
            StatusCode::SERVICE_UNAVAILABLE,
            page(Some(&full.to_string())),
        )
            .into_response(),
    }
}

/// The client's address: the socket's peer, or behind a reverse proxy on this machine (Caddy on
/// the VPS) the address it put last in `X-Forwarded-For`.
fn client_ip(request: &Request) -> IpAddr {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_canonical());
    let forwarded = || {
        request
            .headers()
            .get("x-forwarded-for")?
            .to_str()
            .ok()?
            .rsplit(',')
            .next()?
            .trim()
            .parse::<IpAddr>()
            .ok()
    };
    match peer {
        Some(ip) if ip.is_loopback() => forwarded().unwrap_or(ip),
        Some(ip) => ip,
        None => IpAddr::from([0, 0, 0, 0]),
    }
}

/// The landing page, with `notice` above the buttons. Only fixed text and server messages go in.
pub fn page(notice: Option<&str>) -> Response {
    let notice = notice
        .map(|n| format!("<p class=\"notice\">{}</p>", escape(n)))
        .unwrap_or_default();
    let html = PAGE.replace("{notice}", &notice);
    ([(header::CACHE_CONTROL, "no-store")], Html(html)).into_response()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Among Azeroth</title>
<style>
  :root { color-scheme: dark; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center;
         background: #14110d; color: #e8dcc0; font: 16px/1.5 Georgia, serif; }
  main { width: min(26rem, calc(100% - 2rem)); }
  h1 { font-size: 2rem; margin: 0 0 1.5rem; color: #f2c14e; }
  form { display: flex; gap: .5rem; margin: 0 0 1rem; }
  button, input { font: inherit; padding: .6rem .9rem; border-radius: 4px; border: 1px solid #6b5a3a; }
  button { background: #7a1f12; color: #fff3d6; cursor: pointer; }
  button.wide { width: 100%; }
  input { flex: 1; min-width: 0; background: #241e16; color: inherit; text-transform: uppercase; }
  .notice { background: #3a2412; border-left: 3px solid #f2c14e; padding: .6rem .8rem; }
  .how { color: #b9ab8c; font-size: .9rem; }
</style>
</head>
<body>
<main>
  <h1>Among Azeroth</h1>
  {notice}
  <form method="post" action="/lobby/new">
    <button class="wide" type="submit">Create lobby</button>
  </form>
  <form method="get" action="/">
    <input name="lobby" placeholder="Lobby code" maxlength="16" pattern="[A-Za-z0-9]+" required
           autocomplete="off" aria-label="Lobby code">
    <button type="submit">Join</button>
  </form>
  <p class="how">Your lobby's link ends in <code>?lobby=CODE</code>: share it with friends.
  In the game, log in with any account name and the lobby code as the password.
  A lobby closes after ten minutes with nobody in it.</p>
  <p class="how">Native client: set the realmlist to this server and log in as
  <code>NAME-CODE</code> with the code as the password.</p>
</main>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_makes_three_lobbies_per_window() {
        let limiter = Limiter::default();
        let ip = IpAddr::from([203, 0, 113, 7]);
        let now = Instant::now();
        assert!((0..3).all(|_| limiter.allow(ip, now)));
        assert!(!limiter.allow(ip, now));
        assert!(
            limiter.allow(IpAddr::from([203, 0, 113, 8]), now),
            "per address"
        );
        assert!(limiter.allow(ip, now + WINDOW), "the window moves on");
    }

    #[test]
    fn behind_a_local_proxy_the_forwarded_address_counts() {
        let mut request = Request::new(axum::body::Body::empty());
        request
            .headers_mut()
            .insert("x-forwarded-for", "198.51.100.4".parse().unwrap());
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5000))));
        assert_eq!(client_ip(&request), IpAddr::from([198, 51, 100, 4]));
        // A direct client cannot claim another address.
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 5000))));
        assert_eq!(client_ip(&request), IpAddr::from([203, 0, 113, 9]));
    }

    #[test]
    fn the_notice_is_escaped() {
        assert_eq!(escape("<b>&\""), "&lt;b&gt;&amp;&quot;");
    }
}
