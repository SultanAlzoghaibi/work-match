// Localhost endpoint the Chrome extension posts tab text to. Only the
// extension's origin is accepted; text is scrubbed before it is stored.
use crate::buffer::{self, Buffer, NewCapture};
use crate::scrub::scrub;
use crate::sharing::Sharing;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::net::Ipv4Addr;
use std::sync::Arc;
use url::Url;

pub const PORT: u16 = 47321;
/// ID derived from the "key" in extension/manifest.json (see extension/README.md).
const DEFAULT_EXTENSION_ID: &str = "cifmncfppiahnojgipabfaaifahknecg";
const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct CaptureState {
    pub buffer: Buffer,
    pub sharing: Sharing,
    pub allowed_origin: Arc<str>,
}

#[derive(Deserialize)]
struct CaptureRequest {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, PartialEq)]
pub struct PageUrl {
    pub host: String,
    pub path: String,
}

/// Only http(s) pages are accepted. Everything but host and path (port,
/// credentials, query string, fragment) is dropped.
pub fn normalize_url(raw: &str) -> Option<PageUrl> {
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    Some(PageUrl {
        host: url.host_str()?.to_string(),
        path: url.path().to_string(),
    })
}

pub fn allowed_origin() -> String {
    let id = std::env::var("ALLOWED_EXTENSION_ID")
        .unwrap_or_else(|_| DEFAULT_EXTENSION_ID.to_string());
    format!("chrome-extension://{id}")
}

pub fn router(state: CaptureState) -> Router {
    Router::new()
        .route("/capture", post(capture).options(preflight))
        .with_state(state)
}

/// 0 = starting, 1 = listening, 2 = couldn't bind (another instance has the port).
static LISTENING: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// For the Dev tab.
pub fn listening_status() -> &'static str {
    match LISTENING.load(std::sync::atomic::Ordering::Relaxed) {
        1 => "listening",
        2 => "failed",
        _ => "starting",
    }
}

/// Serves on 127.0.0.1 only, never on other interfaces.
pub fn start(buffer: Buffer, sharing: Sharing) {
    let state = CaptureState {
        buffer,
        sharing,
        allowed_origin: allowed_origin().into(),
    };
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, PORT)).await {
            Ok(listener) => listener,
            Err(err) => {
                LISTENING.store(2, std::sync::atomic::Ordering::Relaxed);
                eprintln!("[capture] can't listen on 127.0.0.1:{PORT}: {err}");
                return;
            }
        };
        LISTENING.store(1, std::sync::atomic::Ordering::Relaxed);
        println!("[capture] listening on 127.0.0.1:{PORT}");
        if let Err(err) = axum::serve(listener, router(state)).await {
            eprintln!("[capture] server stopped: {err}");
        }
    });
}

fn has_allowed_origin(headers: &HeaderMap, allowed: &str) -> bool {
    headers
        .get(header::ORIGIN)
        .is_some_and(|origin| origin.as_bytes() == allowed.as_bytes())
}

// The checks run in this order on purpose: nothing is read from a request
// that isn't from the extension, and nothing is parsed while sharing is off.
async fn capture(State(state): State<CaptureState>, headers: HeaderMap, body: Body) -> StatusCode {
    if !has_allowed_origin(&headers, &state.allowed_origin) {
        return StatusCode::FORBIDDEN;
    }
    if !state.sharing.is_on() {
        return StatusCode::NO_CONTENT;
    }
    let Ok(bytes) = axum::body::to_bytes(body, MAX_BODY_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE;
    };
    let Ok(request) = serde_json::from_slice::<CaptureRequest>(&bytes) else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(page) = normalize_url(&request.url) else {
        return StatusCode::BAD_REQUEST;
    };

    let path = scrub(&page.path);
    let title = scrub(&request.title);
    let text = scrub(&request.text);
    let text_hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    let new_capture = NewCapture {
        captured_at: buffer::now_ms(),
        source: "chrome",
        host: &page.host,
        path: &path,
        title: &title,
        text: &text,
        text_hash: &text_hash,
    };
    match state.buffer.insert(&new_capture) {
        Ok(true) => println!("[capture] {} {} chars", page.host, text.chars().count()),
        Ok(false) => {}
        Err(err) => {
            eprintln!("[capture] insert failed: {err}");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
    StatusCode::NO_CONTENT
}

// Extensions with host permission skip CORS, so this only matters if Chrome
// ever preflights the request (including Local Network Access checks).
async fn preflight(State(state): State<CaptureState>, headers: HeaderMap) -> Response {
    if !has_allowed_origin(&headers, &state.allowed_origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    (
        StatusCode::NO_CONTENT,
        [
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, headers[header::ORIGIN].clone()),
            (header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("POST")),
            (header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type")),
            (
                HeaderName::from_static("access-control-allow-private-network"),
                HeaderValue::from_static("true"),
            ),
        ],
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    const ORIGIN: &str = "chrome-extension://testextensionid";

    fn test_state(sharing_on: bool) -> CaptureState {
        CaptureState {
            buffer: Buffer::open_in_memory(),
            sharing: Sharing::new(sharing_on),
            allowed_origin: ORIGIN.into(),
        }
    }

    async fn post_capture(state: &CaptureState, origin: Option<&str>, body: String) -> StatusCode {
        let mut request = Request::post("/capture").header(header::CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let request = request.body(Body::from(body)).unwrap();
        router(state.clone()).oneshot(request).await.unwrap().status()
    }

    fn capture_json(url: &str, text: &str) -> String {
        serde_json::json!({
            "url": url,
            "title": "Course Schedule - LeetCode",
            "text": text,
            "captured_at": 1,
        })
        .to_string()
    }

    fn row_count(state: &CaptureState) -> usize {
        state.buffer.read_all().unwrap().len()
    }

    #[tokio::test]
    async fn rejects_missing_or_wrong_origin() {
        let state = test_state(true);
        let body = capture_json("https://leetcode.com/problems/course-schedule/", "graphs");
        assert_eq!(post_capture(&state, None, body.clone()).await, StatusCode::FORBIDDEN);
        assert_eq!(
            post_capture(&state, Some("https://evil.example"), body.clone()).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            post_capture(&state, Some("chrome-extension://otherextension"), body).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(row_count(&state), 0);
    }

    #[tokio::test]
    async fn stores_nothing_while_sharing_is_off() {
        let state = test_state(false);
        let body = capture_json("https://leetcode.com/problems/course-schedule/", "graphs");
        assert_eq!(post_capture(&state, Some(ORIGIN), body).await, StatusCode::NO_CONTENT);
        assert_eq!(row_count(&state), 0);
    }

    #[tokio::test]
    async fn rejects_non_web_urls_and_bad_json() {
        let state = test_state(true);
        for url in ["file:///etc/passwd", "chrome://settings", "chrome-extension://abc/page.html"] {
            let body = capture_json(url, "text");
            assert_eq!(post_capture(&state, Some(ORIGIN), body).await, StatusCode::BAD_REQUEST, "{url}");
        }
        assert_eq!(
            post_capture(&state, Some(ORIGIN), "not json".into()).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(row_count(&state), 0);
    }

    #[tokio::test]
    async fn rejects_oversized_bodies() {
        let state = test_state(true);
        let body = capture_json("https://leetcode.com/", &"a".repeat(MAX_BODY_BYTES));
        assert_eq!(post_capture(&state, Some(ORIGIN), body).await, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(row_count(&state), 0);
    }

    #[tokio::test]
    async fn stores_scrubbed_text_without_query_and_skips_duplicates() {
        let state = test_state(true);
        let body = capture_json(
            "https://LeetCode.com:443/problems/course-schedule/?envType=study#description",
            "Ping me at jane@example.com  about   course schedule",
        );
        assert_eq!(post_capture(&state, Some(ORIGIN), body.clone()).await, StatusCode::NO_CONTENT);
        assert_eq!(post_capture(&state, Some(ORIGIN), body).await, StatusCode::NO_CONTENT);

        let rows = state.buffer.read_all().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].host, "leetcode.com");
        assert_eq!(rows[0].path, "/problems/course-schedule/");
        assert_eq!(rows[0].text, "Ping me at [email] about course schedule");
    }

    async fn preflight_status(state: &CaptureState, origin: &str) -> StatusCode {
        let request = Request::options("/capture")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap();
        router(state.clone()).oneshot(request).await.unwrap().status()
    }

    #[tokio::test]
    async fn answers_preflight_only_for_the_extension() {
        let state = test_state(true);
        assert_eq!(preflight_status(&state, ORIGIN).await, StatusCode::NO_CONTENT);
        assert_eq!(preflight_status(&state, "https://evil.example").await, StatusCode::FORBIDDEN);
    }

    #[test]
    fn normalize_url_keeps_only_host_and_path() {
        assert_eq!(
            normalize_url("https://user:pw@www.Zillow.com:8443/homes/Oakland/?price=2000#map"),
            Some(PageUrl {
                host: "www.zillow.com".into(),
                path: "/homes/Oakland/".into()
            })
        );
        assert_eq!(normalize_url("http://example.com").unwrap().path, "/");
        for url in ["file:///etc/passwd", "chrome://settings", "javascript:alert(1)", "data:text/html,hi", "not a url"] {
            assert_eq!(normalize_url(url), None, "{url}");
        }
    }
}
