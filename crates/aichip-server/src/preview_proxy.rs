//! Reach a preview by name instead of by port.
//!
//! `http://fix-the-login-a1b2c3.preview.localhost:4820` reaches the container
//! that card is running in. Two things this buys that a port cannot:
//!
//! * **The link survives.** A port is asked of the OS each time a preview
//!   starts, so a rebuild or a wake hands back a different one — the tab you
//!   left open points at nothing, or at whatever took the port meanwhile.
//! * **Previews stop sharing a cookie jar.** Cookies ignore ports, so two
//!   previews on `127.0.0.1` are one origin as far as the browser's cookie
//!   store is concerned: log into one and the other is logged in too. Distinct
//!   hostnames are what actually keeps them apart.
//!
//! `*.localhost` resolves to loopback without any `/etc/hosts` entry — checked
//! on this machine, not assumed — so this needs no setup and no DNS.
//!
//! ## Why this is not a hole in the dashboard's front door
//!
//! `reject_non_local_callers` refuses any request whose `Host` is not loopback,
//! which is what stops a container or a page on another origin from driving
//! aichip's API. This runs *before* that check and handles preview hosts
//! entirely, never calling into the dashboard router — so a preview hostname
//! can reach a preview and nothing else. The suffix match is on the whole host,
//! so `evil.preview.localhost.attacker.com` does not match, and the label
//! itself may not contain a dot.

use crate::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, Request, Response, StatusCode};
use axum::middleware::Next;
use sqlx::Row;

/// The suffix a preview hostname must end with, exactly.
pub const PREVIEW_SUFFIX: &str = ".preview.localhost";

/// The preview slug in a `Host` value, if this is a preview hostname at all.
///
/// Returns `None` for everything else, which is how the dashboard keeps
/// answering on `localhost` — this is a routing decision before it is a
/// security one, but it is both.
pub fn slug_of_host(host: &str) -> Option<&str> {
    let bare = crate::bare_host(host);
    let label = bare.strip_suffix(PREVIEW_SUFFIX)?;
    // One label, and a real one. A dot here would mean a longer name that
    // merely ends with our suffix — `x.preview.localhost.attacker.com` cannot
    // reach this, but `a.b.preview.localhost` should not either.
    if label.is_empty() || label.contains('.') {
        return None;
    }
    Some(label)
}

/// Headers that describe one hop and must not be copied to the next.
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn forwardable(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        let n = name.as_str().to_ascii_lowercase();
        // `host` is rewritten by the client to the upstream authority; copying
        // ours would tell the container it is being served at a name it does
        // not know.
        if n == "host" || HOP_BY_HOP.contains(&n.as_str()) {
            continue;
        }
        out.insert(name.clone(), value.clone());
    }
    out
}

/// Serve preview hostnames; hand everything else to the dashboard.
pub async fn route_previews(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let Some(slug) = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| slug_of_host(h).map(str::to_string))
    else {
        return next.run(req).await;
    };

    match proxy(&state, &slug, req).await {
        Ok(response) => response,
        Err(message) => plain(StatusCode::BAD_GATEWAY, message),
    }
}

async fn proxy(
    state: &AppState,
    slug: &str,
    req: Request<Body>,
) -> Result<Response<Body>, String> {
    // Only a *running* preview answers. A slug belonging to one that is idle
    // or stopped gets a page saying so rather than a connection refused, which
    // is the difference between "it went to sleep" and "aichip is broken".
    let row = sqlx::query(
        "SELECT status, host_port FROM previews WHERE slug = $1
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(slug)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| format!("could not look up this preview: {e}"))?;

    let Some(row) = row else {
        return Ok(plain(
            StatusCode::NOT_FOUND,
            format!("No preview is called \"{slug}\". It may have been stopped."),
        ));
    };
    let status: String = row.get("status");
    let Some(port) = row.get::<Option<i32>, _>("host_port").filter(|_| status == "running") else {
        return Ok(plain(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "This preview is {status}. Open its card in aichip and start it again — \
                 if its image is still here that takes a few seconds."
            ),
        ));
    };

    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let url = format!("http://127.0.0.1:{port}{path}");
    let method = req.method().clone();
    let headers = forwardable(req.headers());

    // Buffered rather than streamed. A preview serves pages and assets, and
    // the simplicity is worth more here than the memory would be — noted
    // rather than hidden, because a preview that streams a large download
    // would hold it all in memory on the way through.
    let body = axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024)
        .await
        .map_err(|_| "the request body was too large to proxy".to_string())?;

    let upstream = reqwest::Client::new()
        .request(method, &url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|e| {
            format!("This preview's container is not answering on port {port}. ({e})")
        })?;

    let mut out = Response::builder().status(upstream.status());
    for (name, value) in upstream.headers() {
        if HOP_BY_HOP.contains(&name.as_str().to_ascii_lowercase().as_str()) {
            continue;
        }
        out = out.header(name, value);
    }
    let bytes = upstream
        .bytes()
        .await
        .map_err(|e| format!("this preview's response could not be read: {e}"))?;
    out.body(Body::from(bytes))
        .map_err(|e| format!("could not assemble the proxied response: {e}"))
}

fn plain(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(HeaderName::from_static("content-type"), "text/plain; charset=utf-8")
        .body(Body::from(message.into()))
        .expect("a plain-text response is always well formed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_a_single_label_under_our_own_suffix() {
        assert_eq!(slug_of_host("card-a.preview.localhost"), Some("card-a"));
        assert_eq!(slug_of_host("card-a.preview.localhost:4820"), Some("card-a"));
        assert_eq!(
            slug_of_host("http://card-a.preview.localhost:4820/"),
            Some("card-a")
        );
    }

    #[test]
    fn refuses_names_that_merely_contain_the_suffix() {
        // The attack this is here for: a name the attacker controls that ends
        // up looking like ours to a sloppy `contains`.
        assert_eq!(slug_of_host("card-a.preview.localhost.attacker.com"), None);
        assert_eq!(slug_of_host("preview.localhost.evil.test"), None);
        // Nested labels are not ours either.
        assert_eq!(slug_of_host("a.b.preview.localhost"), None);
        // The dashboard's own hosts must fall through, not be treated as slugs.
        assert_eq!(slug_of_host("localhost:4820"), None);
        assert_eq!(slug_of_host("127.0.0.1:4820"), None);
        assert_eq!(slug_of_host("[::1]:4820"), None);
        // An empty label is not a name.
        assert_eq!(slug_of_host(".preview.localhost"), None);
        assert_eq!(slug_of_host("preview.localhost"), None);
    }

    #[test]
    fn strips_headers_that_describe_only_this_hop() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "card-a.preview.localhost".parse().unwrap());
        headers.insert("connection", "keep-alive".parse().unwrap());
        headers.insert("accept", "text/html".parse().unwrap());
        headers.insert("cookie", "session=abc".parse().unwrap());
        let out = forwardable(&headers);
        assert!(out.get("host").is_none());
        assert!(out.get("connection").is_none());
        // The preview's own cookies must survive, or nothing with a login works.
        assert_eq!(out.get("cookie").unwrap(), "session=abc");
        assert_eq!(out.get("accept").unwrap(), "text/html");
    }
}
