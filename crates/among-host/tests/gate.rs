//! The lobby gate as a router: no live code is 403 (or the landing page at `/`), an invite link
//! sets the cookie, the cookie lets in, a created lobby's code works until the lobby closes, and
//! the game page carries the script that says so once it has.

use std::net::SocketAddr;
use std::time::Duration;

use among_server::{Accounts, Lobbies};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::response::{Html, Response};
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;

fn lobbies(max: usize) -> Lobbies {
    let lobbies = Lobbies::new(
        among_server::lobbies::Template {
            settings: among_rules::Settings::default(),
            layout: among_server::game::placeholder_layout(),
            look: among_server::game::placeholder_look(),
            start_countdown: Duration::ZERO,
        },
        max,
        Duration::from_secs(600),
    );
    lobbies.add_fixed(Accounts::open_lobby("K3XQ9P").unwrap());
    lobbies
}

fn app(lobbies: &Lobbies) -> Router {
    among_host::app(
        Router::new()
            .route("/data/__index", get(|| async { "files" }))
            .route("/", get(|| async { "game page" })),
        lobbies.clone(),
        among_host::Limiter::default(),
    )
}

async fn get_with(lobbies: &Lobbies, uri: &str, cookie: Option<&str>) -> Response {
    let mut request = Request::get(uri);
    if let Some(c) = cookie {
        request = request.header(header::COOKIE, c);
    }
    app(lobbies)
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// `POST /lobby/new` from `ip`, through `app`, so its limiter is shared between calls.
async fn create(app: &Router, ip: [u8; 4]) -> Response {
    let mut request = Request::post("/lobby/new").body(Body::empty()).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((ip, 40000))));
    app.clone().oneshot(request).await.unwrap()
}

async fn text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn game_files_need_a_live_lobby_code() {
    let lobbies = lobbies(8);
    assert_eq!(
        get_with(&lobbies, "/data/__index", None).await.status(),
        StatusCode::FORBIDDEN
    );
    let wrong = get_with(&lobbies, "/data/__index", Some("among_lobby=AAAAAA")).await;
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_invite_link_sets_the_cookie_that_lets_later_requests_in() {
    let lobbies = lobbies(8);
    let first = get_with(&lobbies, "/data/__index?lobby=k3xq9p", None).await;
    assert_eq!(first.status(), StatusCode::OK);
    let cookie = first.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("among_lobby=K3XQ9P;"), "{cookie}");
    let later = get_with(&lobbies, "/data/__index", Some("among_lobby=K3XQ9P")).await;
    assert_eq!(later.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_bare_root_is_the_landing_page_and_a_live_code_the_game() {
    let lobbies = lobbies(8);
    let landing = get_with(&lobbies, "/", None).await;
    assert_eq!(landing.status(), StatusCode::OK);
    let page = text(landing).await;
    assert!(
        page.contains("Create lobby") && page.contains("Join"),
        "{page}"
    );
    let game = get_with(&lobbies, "/?lobby=K3XQ9P", None).await;
    assert_eq!(text(game).await, "game page");
    let unknown = get_with(&lobbies, "/?lobby=ZZZZZZ", None).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert!(text(unknown).await.contains("Lobby ZZZZZZ is closed"));
}

#[tokio::test]
async fn a_bare_root_is_the_landing_page_even_with_a_live_lobby_cookie() {
    let lobbies = lobbies(8);
    let page = text(get_with(&lobbies, "/", Some("among_lobby=K3XQ9P")).await).await;
    assert!(page.contains("Create lobby"), "{page}");
}

#[tokio::test]
async fn a_created_lobby_lets_in_until_it_closes() {
    let lobbies = lobbies(8);
    let app = app(&lobbies);
    let made = create(&app, [203, 0, 113, 7]).await;
    assert_eq!(made.status(), StatusCode::SEE_OTHER);
    let location = made.headers()[header::LOCATION].to_str().unwrap();
    let code = location
        .strip_prefix("/?lobby=")
        .expect(location)
        .to_owned();
    assert!(lobbies.open(&code).is_some());

    let link = get_with(&lobbies, location, None).await;
    assert_eq!(text(link).await, "game page");
    let cookie = format!("among_lobby={code}");
    let files = get_with(&lobbies, "/data/__index", Some(&cookie)).await;
    assert_eq!(files.status(), StatusCode::OK);

    // Empty past the idle time, it closes, and its code no longer opens anything.
    let closed = lobbies.sweep(tokio::time::Instant::now() + Duration::from_secs(601));
    assert_eq!(closed, vec![code.clone()]);
    let files = get_with(&lobbies, "/data/__index", Some(&cookie)).await;
    assert_eq!(files.status(), StatusCode::FORBIDDEN);
    let link = get_with(&lobbies, location, None).await;
    assert_eq!(link.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn creation_stops_at_the_cap_and_per_address() {
    let lobbies = lobbies(3);
    let app = app(&lobbies);
    assert_eq!(
        create(&app, [203, 0, 113, 1]).await.status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        create(&app, [203, 0, 113, 2]).await.status(),
        StatusCode::SEE_OTHER
    );
    let full = create(&app, [203, 0, 113, 3]).await;
    assert_eq!(full.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(text(full).await.contains("All 3 lobbies"));

    let roomy = self::lobbies(8);
    let app = self::app(&roomy);
    for _ in 0..3 {
        assert_eq!(
            create(&app, [198, 51, 100, 9]).await.status(),
            StatusCode::SEE_OTHER
        );
    }
    let limited = create(&app, [198, 51, 100, 9]).await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(roomy.len(), 4, "the fixed lobby and three made ones");
}

/// A game page like wenilla's `index.html`: it says whether it was asked for compressed.
fn page_app(lobbies: &Lobbies) -> Router {
    let page = |headers: axum::http::HeaderMap| async move {
        let asked = headers.contains_key(header::ACCEPT_ENCODING);
        Html(format!(
            "<!doctype html><meta charset=utf-8><p>gzip asked: {asked}</p><script>boot()</script>"
        ))
    };
    among_host::app(
        Router::new()
            .route("/", get(page))
            .route("/data/__index", get(|| async { "files" })),
        lobbies.clone(),
        among_host::Limiter::default(),
    )
}

#[tokio::test]
async fn the_game_page_carries_the_closed_lobby_notice_before_its_own_scripts() {
    let lobbies = lobbies(8);
    let request = Request::get("/?lobby=K3XQ9P")
        .header(header::ACCEPT_ENCODING, "br, gzip")
        .body(Body::empty())
        .unwrap();
    let response = page_app(&lobbies).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = text(response).await;
    assert!(page.contains("gzip asked: false"), "{page}");
    let notice = page.find("This lobby has closed").expect(&page);
    assert!(notice < page.find("boot()").unwrap(), "{page}");
    assert!(page.starts_with("<!doctype html>"), "{page}");

    // The files behind it still answer as they did, and a closed lobby's still 403.
    let files = Request::get("/data/__index")
        .header(header::COOKIE, "among_lobby=K3XQ9P")
        .body(Body::empty())
        .unwrap();
    let files = page_app(&lobbies).oneshot(files).await.unwrap();
    assert_eq!(text(files).await, "files");
    let closed = Request::get("/data/__index")
        .header(header::COOKIE, "among_lobby=AAAAAA")
        .body(Body::empty())
        .unwrap();
    let closed = page_app(&lobbies).oneshot(closed).await.unwrap();
    assert_eq!(closed.status(), StatusCode::FORBIDDEN);
}
