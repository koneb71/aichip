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
use tokio::sync::broadcast::error::RecvError;
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
        if replay_since(&mut socket, &state, run_id, &mut last_seq)
            .await
            .is_err()
        {
            return;
        }
    }

    loop {
        tokio::select! {
            envelope = live.recv() => {
                let envelope = match envelope {
                    Ok(envelope) => envelope,
                    // The bus is one ring shared by every run, and it moved on
                    // past events this socket had not read yet. That used to
                    // end the stream — and the dashboard, which never
                    // reconnected, froze on whatever it last showed. Nothing
                    // is actually lost: every envelope is persisted before it
                    // is published, so the log already holds what the ring
                    // dropped. Catch up from it and carry on.
                    Err(RecvError::Lagged(skipped)) => {
                        tracing::debug!(skipped, run_id = ?params.run_id, "ws lagged; replaying from the log");
                        if let Some(run_id) = params.run_id {
                            if replay_since(&mut socket, &state, run_id, &mut last_seq)
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        continue;
                    }
                    Err(RecvError::Closed) => break,
                };
                if let Some(run_id) = params.run_id {
                    if envelope.run_id != run_id {
                        continue;
                    }
                    // Skip events already delivered during replay (permission
                    // events use seq -1 and always pass through).
                    if envelope.seq >= 0 && envelope.seq <= last_seq {
                        continue;
                    }
                    // `last_seq` is deliberately not advanced here. Steps of
                    // one run share an allocator but publish independently,
                    // so seq 11 can arrive before seq 10; a watermark moved by
                    // the live tail would drop 10. Only a replay — which reads
                    // the log in order — may move it.
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

/// Send every persisted event of `run_id` past `last_seq`, advancing it.
///
/// The one path from the log to a socket, used at connect and again whenever
/// the live tail falls behind. `Err` means the socket is gone.
async fn replay_since(
    socket: &mut WebSocket,
    state: &AppState,
    run_id: Uuid,
    last_seq: &mut i64,
) -> Result<(), ()> {
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
    .bind(*last_seq)
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
            return Err(());
        }
        *last_seq = (*last_seq).max(seq);
    }
    Ok(())
}
