pub mod app_bridge;
pub mod mcp;
pub mod preview_proxy;
pub mod routes;
pub mod ws;

use aichip_core::runs::permissions::PermissionBroker;
use aichip_core::{Db, EventBus, Orchestrator};
use axum::http::{HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::Router;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub bus: EventBus,
    pub orchestrator: Arc<Orchestrator>,
    pub permissions: PermissionBroker,
    /// Object storage for knowledge-base attachments. `None` when it isn't
    /// configured, which is a normal state: articles work without it, and the
    /// upload endpoint says so rather than failing obscurely.
    pub storage: Option<aichip_core::storage::Storage>,
    /// Serializes file saves from the Files tab.
    ///
    /// A save is read-hash-compare-write, and without a lock two of them can
    /// interleave so the second lands on bytes the first never saw — exactly
    /// the clobber `baseHash` exists to prevent. Saves are human-paced, so one
    /// lock for all of them is free.
    pub file_writes: Arc<tokio::sync::Mutex<()>>,
}

pub fn app(state: AppState) -> Router {
    let mut router = Router::new()
        .nest("/api", routes::api_router())
        .nest("/mcp", mcp::mcp_router())
        .route("/ws", axum::routing::get(ws::ws_handler))
        // The capability bridge exists on app hostnames only, where the proxy
        // layer above answers it and never calls through to here. Saying so
        // explicitly, because the SPA fallback would otherwise serve
        // `index.html` with a 200 for `/__aichip/me` on localhost — no
        // capability leaked, but a very confusing thing to find while probing.
        .route("/__aichip", axum::routing::any(no_bridge_here))
        .route("/__aichip/{*rest}", axum::routing::any(no_bridge_here));

    // Dashboard assets: AICHIP_WEB_DIST overrides; defaults to ./web/dist
    // for dev checkouts. (v1.0 embeds these in the binary via rust-embed.)
    let dist = std::env::var("AICHIP_WEB_DIST").unwrap_or_else(|_| "web/dist".into());
    if std::path::Path::new(&dist).join("index.html").exists() {
        let serve = tower_http::services::ServeDir::new(&dist)
            .fallback(tower_http::services::ServeFile::new(
                std::path::Path::new(&dist).join("index.html"),
            ));
        router = router.fallback_service(serve);
    }

    router
        .layer(middleware::from_fn(reject_non_local_callers))
        // Inside the preview proxy and outside the loopback check, so every
        // dashboard response carries it — including the 403 above — and no
        // proxied response does. A preview must stay framable; the dashboard
        // must not.
        .layer(middleware::from_fn(refuse_to_be_framed))
        // Outside the loopback check on purpose: a preview hostname is handled
        // here in full and never reaches the dashboard router, so widening what
        // `Host` values are accepted does not widen what can reach the API.
        // See `preview_proxy` for why the suffix match is safe.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            preview_proxy::route_previews,
        ))
        .with_state(state)
}

async fn no_bridge_here() -> (StatusCode, &'static str) {
    (
        StatusCode::NOT_FOUND,
        "The app bridge is served on an app's own hostname, not here.",
    )
}

/// Refuse to be displayed inside anyone else's frame.
///
/// aichip has no authentication, so every control on this page acts the moment
/// it is clicked: a mid-run permission prompt's **Allow**, a Dockerfile
/// approval, a squash-merge. Nothing stopped another page from loading the
/// dashboard in an invisible iframe, positioning it under something innocuous,
/// and collecting one of those clicks — the textbook clickjack, against a UI
/// made of one-click irreversible actions.
///
/// Both headers, because they are not the same check: `frame-ancestors` is the
/// one browsers actually honour now, and `X-Frame-Options` still covers what
/// predates it. Neither reaches a preview or an app, which are *meant* to be
/// embedded — that is what the layer's position buys.
async fn refuse_to_be_framed(
    req: Request<axum::body::Body>,
    next: Next,
) -> axum::response::Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        axum::http::header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    res
}

/// Hosts this server will answer to, and origins whose pages may call it.
const LOCAL_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

/// The host part of a `Host` or `Origin` value, without the port.
///
/// Written out rather than `split(':').next()`, which the previous version used
/// and which is wrong for IPv6: `"[::1]:4820".split(':').next()` is `"["`, so
/// the `[::1]` arm of the old allowlist had never once matched.
pub(crate) fn bare_host(value: &str) -> &str {
    let authority = value.rsplit_once("://").map_or(value, |(_, rest)| rest);
    let authority = authority.split('/').next().unwrap_or("");
    match authority.strip_prefix('[') {
        // IPv6 literal: the port, if any, follows the closing bracket.
        Some(rest) => match rest.split_once(']') {
            Some((inner, _)) => {
                // Return the bracketed form, which is how a Host header spells it.
                let end = inner.len() + 2;
                &authority[..end.min(authority.len())]
            }
            None => authority,
        },
        None => authority.split(':').next().unwrap_or(""),
    }
}

/// True when a page at this origin is allowed to talk to the dashboard.
///
/// Port-agnostic, deliberately: `vite dev` serves the dashboard from :5173 and
/// proxies `/api` and `/ws` through to this server, so the browser's origin is
/// `http://localhost:5173` and not the port aichip is listening on. Requiring an
/// exact match would break every dev checkout.
///
/// Being loose about the port costs nothing that matters. To serve a page from a
/// loopback origin an attacker must already be running code on this machine, and
/// at that point they can talk to this server directly without a browser.
///
/// An exact host match, never a suffix: when deployments arrive they get their own
/// `<slug>.localhost` names, and a preview running code an agent wrote must not be
/// able to call the dashboard's API from a page.
fn origin_is_local(origin: &str) -> bool {
    // `Origin: null` is what a sandboxed iframe and some redirect chains send.
    // It is not a local page; it is the absence of one.
    origin != "null" && LOCAL_HOSTS.contains(&bare_host(origin))
}

/// Refuse callers that are not this machine's own dashboard.
///
/// Two checks, against two different attacks.
///
/// **Host** is the DNS-rebinding defence this has always had: the server binds
/// 127.0.0.1, and it also declines to answer to a name it does not recognise.
///
/// **Origin** is new, and closes a hole that was open. aichip has no
/// authentication of any kind, so until now any page on the internet could open
/// `ws://localhost:4820/ws` and read every run's transcript — prompts, file
/// contents, costs — and could POST to the mutating endpoints that take no JSON
/// body. Verified before the fix: a WebSocket upgrade carrying
/// `Origin: https://evil.example` was answered with `101 Switching Protocols`.
///
/// A missing `Origin` is allowed, and has to be: the spawned agent CLIs call
/// `/mcp`, and `aichip doctor` and any curl call `/api`, none of them browsers and
/// none of them sending one. That is not a weakness — a program that can set
/// arbitrary headers is not the attacker this check is for. The attacker here is a
/// web page, and browsers attach `Origin` to exactly the cross-origin requests
/// that matter.
async fn reject_non_local_callers(
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<axum::response::Response, StatusCode> {
    // Both decisions are made before `req` is handed on, so neither borrow of
    // its headers is still alive at the move.
    let (host_ok, origin_ok) = {
        let headers = req.headers();
        let str_of = |name| headers.get(name).and_then(|v: &HeaderValue| v.to_str().ok());
        let host = str_of(axum::http::header::HOST).unwrap_or_default();
        (
            LOCAL_HOSTS.contains(&bare_host(host)),
            str_of(axum::http::header::ORIGIN).is_none_or(origin_is_local),
        )
    };

    if host_ok && origin_ok {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_hosts_keep_their_brackets_and_lose_their_port() {
        // The bug this replaces: the old `split(':').next()` returned "[".
        assert_eq!(bare_host("[::1]:4820"), "[::1]");
        assert_eq!(bare_host("[::1]"), "[::1]");
        assert!(LOCAL_HOSTS.contains(&bare_host("[::1]:4820")));
    }

    #[test]
    fn ordinary_hosts_lose_their_port() {
        assert_eq!(bare_host("localhost:4820"), "localhost");
        assert_eq!(bare_host("127.0.0.1:4820"), "127.0.0.1");
        assert_eq!(bare_host("localhost"), "localhost");
    }

    #[test]
    fn origins_are_stripped_to_their_host() {
        assert_eq!(bare_host("http://localhost:5173"), "localhost");
        assert_eq!(bare_host("https://evil.example"), "evil.example");
        assert_eq!(bare_host("http://[::1]:4820"), "[::1]");
    }

    #[test]
    fn the_dev_server_and_the_dashboard_are_both_allowed() {
        assert!(origin_is_local("http://localhost:4820"));
        assert!(origin_is_local("http://localhost:5173"));
        assert!(origin_is_local("http://127.0.0.1:4820"));
    }

    #[test]
    fn everything_else_is_not() {
        assert!(!origin_is_local("https://evil.example"));
        assert!(!origin_is_local("null"));
        // A suffix match would let a deployment's own page call the API.
        assert!(!origin_is_local("http://my-preview.localhost:4820"));
        // And a lookalike registered on the public internet must not pass.
        assert!(!origin_is_local("https://localhost.evil.example"));
    }
}
