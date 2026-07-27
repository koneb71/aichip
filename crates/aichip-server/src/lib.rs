pub mod mcp;
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
}

pub fn app(state: AppState) -> Router {
    let mut router = Router::new()
        .nest("/api", routes::api_router())
        .nest("/mcp", mcp::mcp_router())
        .route("/ws", axum::routing::get(ws::ws_handler));

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
        .layer(middleware::from_fn(reject_non_local_hosts))
        .with_state(state)
}

/// DNS-rebinding defense: the server only ever binds 127.0.0.1, and we also
/// refuse requests whose Host header isn't local.
async fn reject_non_local_hosts(
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<axum::response::Response, StatusCode> {
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .map(HeaderValue::as_bytes)
        .unwrap_or_default();
    let host = String::from_utf8_lossy(host);
    let bare = host.split(':').next().unwrap_or("");
    if matches!(bare, "127.0.0.1" | "localhost" | "[::1]") {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
