//! The lobby gate in front of wenilla's browser host. `/data` hands out game files from the host's
//! own install, so every route needs the lobby code: the invite link `/?lobby=CODE` sets it as a
//! cookie, and a request carrying neither gets 403.

use std::io::Write;
use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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

/// `wenilla-host`'s `/data` router with its 404s cacheable like its files: a name absent from the
/// host's chain stays absent, so a browser asks for it once instead of once per load.
pub fn cache_missing(data: Router) -> Router {
    data.layer(axum::middleware::from_fn(mark_missing_immutable))
}

async fn mark_missing_immutable(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    if response.status() == StatusCode::NOT_FOUND {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=31536000, immutable"),
        );
    }
    response
}

/// `router` with every answer held back `delay`: a tunnel's round trip on a local measurement.
pub fn delayed(router: Router, delay: std::time::Duration) -> Router {
    if delay.is_zero() {
        return router;
    }
    router.layer(axum::middleware::from_fn(
        move |request: Request, next: Next| async move {
            tokio::time::sleep(delay).await;
            next.run(request).await
        },
    ))
}

/// One line per `/data` request to `file`, `ms since start <TAB> status <TAB> ms taken <TAB>
/// method <TAB> name`: the browser's real read order, which `crates/among-web/read-set.ps1` cuts
/// the world entry prefetch from.
pub fn request_log(router: Router, file: &Path) -> std::io::Result<Router> {
    let log = Arc::new(RequestLog {
        out: Mutex::new(std::fs::File::create(file)?),
        t0: Instant::now(),
    });
    Ok(router.layer(axum::middleware::from_fn_with_state(log, log_request)))
}

struct RequestLog {
    out: Mutex<std::fs::File>,
    t0: Instant,
}

async fn log_request(State(log): State<Arc<RequestLog>>, request: Request, next: Next) -> Response {
    let Some(raw) = request.uri().path().strip_prefix("/data/") else {
        return next.run(request).await;
    };
    let name = data_name(raw);
    let method = request.method().clone();
    let started = Instant::now();
    let response = next.run(request).await;
    let line = format!(
        "{:.0}\t{}\t{:.1}\t{method}\t{name}\n",
        log.t0.elapsed().as_secs_f64() * 1000.0,
        response.status().as_u16(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    if let Ok(mut out) = log.out.lock() {
        let _ = out.write_all(line.as_bytes());
    }
    response
}

/// The chain name in a `/data/` URL tail: percent-decoded, `/` as `\`, as `wenilla-host` reads it.
fn data_name(raw: &str) -> String {
    percent_encoding::percent_decode_str(raw)
        .decode_utf8_lossy()
        .replace('/', "\\")
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
    fn data_names_decode_like_the_host_reads_them() {
        assert_eq!(
            data_name("World%5CMaps%5CAzeroth%5CAzeroth_32_48.adt"),
            "World\\Maps\\Azeroth\\Azeroth_32_48.adt"
        );
        assert_eq!(data_name("a/b.blp"), "a\\b.blp");
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
