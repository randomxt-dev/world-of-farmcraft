//! The lobby gate as a router: no code is 403, the invite link sets the cookie, the cookie lets in.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;

fn app() -> Router {
    among_host::gated(
        Router::new().route("/data/__index", get(|| async { "files" })),
        "K3XQ9P".into(),
    )
}

async fn get_with(uri: &str, cookie: Option<&str>) -> axum::response::Response {
    let mut request = Request::get(uri);
    if let Some(c) = cookie {
        request = request.header(header::COOKIE, c);
    }
    app()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn game_files_need_the_lobby_code() {
    assert_eq!(
        get_with("/data/__index", None).await.status(),
        StatusCode::FORBIDDEN
    );
    let wrong = get_with("/data/__index", Some("among_lobby=AAAAAA")).await;
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_invite_link_sets_the_cookie_that_lets_later_requests_in() {
    let first = get_with("/data/__index?lobby=k3xq9p", None).await;
    assert_eq!(first.status(), StatusCode::OK);
    let cookie = first.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("among_lobby=K3XQ9P;"), "{cookie}");
    let later = get_with("/data/__index", Some("among_lobby=K3XQ9P")).await;
    assert_eq!(later.status(), StatusCode::OK);
}
