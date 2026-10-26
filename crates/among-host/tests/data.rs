//! The `/data` wrapper: a missing game file is a 404 the browser may cache for good.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;

#[tokio::test]
async fn a_missing_file_is_cached_like_a_present_one() {
    let app = among_host::cache_missing(
        Router::new()
            .route("/data/here.blp", get(|| async { "bytes" }))
            .route("/data/gone_s.blp", get(|| async { StatusCode::NOT_FOUND })),
    );
    let get_uri = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();
    let missing = app
        .clone()
        .oneshot(get_uri("/data/gone_s.blp"))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        missing.headers()[header::CACHE_CONTROL],
        "private, max-age=31536000, immutable"
    );
    let present = app.oneshot(get_uri("/data/here.blp")).await.unwrap();
    assert!(present.headers().get(header::CACHE_CONTROL).is_none());
}
