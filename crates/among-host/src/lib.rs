//! The lobby gate in front of wenilla's browser host. `/data` hands out game files from the host's
//! own install, so every route needs the lobby code: the invite link `/?lobby=CODE` sets it as a
//! cookie, and a request carrying neither gets 403.

use std::net::{IpAddr, UdpSocket};
use std::path::PathBuf;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Router;

const COOKIE: &str = "among_lobby";

/// `router` behind the lobby gate for `code` (uppercase, as the server normalises it).
pub fn gated(router: Router, code: String) -> Router {
    router.layer(axum::middleware::from_fn_with_state(code, gate))
}

async fn gate(State(code): State<String>, request: Request, next: Next) -> Response {
    let from_link = request
        .uri()
        .query()
        .and_then(|q| query_value(q, "lobby"))
        .is_some_and(|c| c.eq_ignore_ascii_case(&code));
    let from_cookie = request
        .headers()
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| cookie_value(v, COOKIE).is_some_and(|c| c == code));
    if !from_link && !from_cookie {
        return (
            StatusCode::FORBIDDEN,
            "Among Azeroth: open the invite link the host gave you (it ends in ?lobby=CODE).\n",
        )
            .into_response();
    }
    let mut response = next.run(request).await;
    if from_link {
        let cookie = format!("{COOKIE}={code}; Path=/; SameSite=Lax; HttpOnly");
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// `name`'s value in a `a=1&b=2` query string.
fn query_value<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(k, v)| (k == name).then_some(v))
}

/// `name`'s value in a `Cookie: a=1; b=2` header.
fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(k, v)| (k == name).then_some(v))
}

/// This machine's LAN address: the source a UDP socket would use for the internet (nothing is sent).
pub fn lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:53").ok()?;
    Some(socket.local_addr().ok()?.ip())
}

/// This machine's Tailscale name (`pc.tailnet.ts.net`), when the `tailscale` CLI is installed and
/// up: `tailscale serve` and `funnel` give it an HTTPS certificate, which WebGPU requires.
pub fn tailscale_name() -> Option<String> {
    let out = std::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .ok()?;
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let name = status["Self"]["DNSName"].as_str()?.trim_end_matches('.');
    (!name.is_empty()).then(|| name.to_owned())
}

/// The quick tunnel's public address in a `cloudflared` log line, when the line announces it
/// (`|  https://<words>.trycloudflare.com  |`); the `api.` host it registers with is not it.
pub fn tunnel_url(line: &str) -> Option<&str> {
    let start = line.find("https://")?;
    let url = line[start..].split_whitespace().next()?;
    (url.ends_with(".trycloudflare.com") && !url.starts_with("https://api.")).then_some(url)
}

/// Where `cloudflared` is: on the PATH, else where its Windows installer (winget
/// `Cloudflare.cloudflared`) puts it.
pub fn cloudflared() -> Option<PathBuf> {
    let on_path = std::process::Command::new("cloudflared")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if on_path {
        return Some(PathBuf::from("cloudflared"));
    }
    ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|dir| {
            PathBuf::from(dir)
                .join("cloudflared")
                .join("cloudflared.exe")
        })
        .find(|exe| exe.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tunnel_url_is_found_in_cloudflareds_banner_and_nowhere_else() {
        let banner =
            "2026-10-14T23:14:50Z INF |  https://supervision-provider.trycloudflare.com  |";
        assert_eq!(
            tunnel_url(banner),
            Some("https://supervision-provider.trycloudflare.com")
        );
        let api = "failed to request quick Tunnel: Post \"https://api.trycloudflare.com/tunnel\"";
        assert_eq!(tunnel_url(api), None);
        assert_eq!(tunnel_url("INF Registered tunnel connection"), None);
    }

    #[test]
    fn query_and_cookie_values_are_found_by_name() {
        assert_eq!(query_value("user=a&lobby=K3XQ9P", "lobby"), Some("K3XQ9P"));
        assert_eq!(query_value("lobbyx=1", "lobby"), None);
        assert_eq!(
            cookie_value("a=1; among_lobby=K3XQ9P", COOKIE),
            Some("K3XQ9P")
        );
        assert_eq!(cookie_value("among_lobby_old=K3XQ9P", COOKIE), None);
    }
}
