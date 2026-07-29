//! WebSocket event streaming with replay. Clients connect with
//! `/ws?run_id=<uuid>&after_seq=<n>`; the server replays persisted events
//! past `after_seq` from the DB, then switches to live bus fan-out. Omitting
//! `run_id` streams live events for all runs (the board's activity tickers).

use crate::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct WsParams {
    run_id: Option<Uuid>,
    #[serde(default = "default_after_seq")]
    after_seq: i64,
}

fn default_after_seq() -> i64 {
    -1
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle(socket, params, state))
}

async fn handle(mut socket: WebSocket, params: WsParams, state: AppState) {
    // Subscribe BEFORE replaying so no events fall in the gap.
    let mut live = state.bus.subscribe();
    let mut last_seq = params.after_seq;

    if let Some(run_id) = params.run_id {
        // `step_id` travels with every replayed frame, matching what the live
        // path already sends. Without it a client can see that *something* is
        // happening but not *who* is doing it — which is why an org run could
        // only ever render status labels. Callers that want names map the id
        // against the step list they already hold.
        let rows = sqlx::query(
            "SELECT seq, payload, ts, step_id FROM events
             WHERE run_id=$1 AND seq > $2 ORDER BY seq ASC",
        )
        .bind(run_id)
        .bind(params.after_seq)
        .fetch_all(&state.db.pool)
        .await
        .unwrap_or_default();
        for row in rows {
            let seq: i64 = row.get("seq");
            let msg = json!({
                "runId": run_id,
                "seq": seq,
                "ts": row.get::<chrono::DateTime<chrono::Utc>, _>("ts"),
                "step_id": row.get::<Option<uuid::Uuid>, _>("step_id"),
                "event": row.get::<serde_json::Value, _>("payload"),
            });
            if socket.send(Message::text(msg.to_string())).await.is_err() {
                return;
            }
            last_seq = last_seq.max(seq);
        }
    }

    loop {
        tokio::select! {
            envelope = live.recv() => {
                let Ok(envelope) = envelope else { break };
                if let Some(run_id) = params.run_id {
                    if envelope.run_id != run_id {
                        continue;
                    }
                    // Skip events already delivered during replay (permission
                    // events use seq -1 and always pass through).
                    if envelope.seq >= 0 && envelope.seq <= last_seq {
                        continue;
                    }
                }
                let Ok(text) = serde_json::to_string(&envelope) else { continue };
                if socket.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
