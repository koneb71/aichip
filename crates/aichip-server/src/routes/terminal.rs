//! A real shell in the project's folder, over a WebSocket.
//!
//! This endpoint is arbitrary code execution *by design* — it is the user
//! opening a terminal on their own machine, the same act as opening
//! Terminal.app and typing `cd`. What makes that safe to expose is the same
//! thing that makes the rest of the dashboard safe: the server binds
//! loopback, and the router-wide Host/Origin guard refuses any web page that
//! is not the dashboard itself. Browsers attach `Origin` to every WebSocket
//! upgrade, so a hostile page cannot open this socket.
//!
//! Two deliberate choices:
//!
//! - The shell gets the user's own login shell with the user's own
//!   environment — except aichip's own secrets (`AICHIP_OWN_SECRETS`),
//!   stripped exactly as they are from every engine child. The user can read
//!   those out of their own process table anyway; stripping them keeps a
//!   pasted `env` screenshot from leaking them by accident.
//! - The session lives exactly as long as the socket. Closing the tab is
//!   closing the terminal window — no orphaned shells accumulating behind a
//!   dashboard nobody remembers detaching from.
//!
//! Wire protocol: binary frames are bytes (both directions); text frames are
//! control JSON, currently only `{"resize":{"cols":N,"rows":N}}`.

use crate::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use sqlx::Row;
use std::io::{Read, Write};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/ws/terminal/{project_id}", get(open))
}

#[derive(Deserialize)]
struct Control {
    resize: Option<Size>,
}

#[derive(Deserialize)]
struct Size {
    cols: u16,
    rows: u16,
}

async fn open(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let path: Option<String> =
        sqlx::query("SELECT path FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(&state.db.pool)
            .await
            .ok()
            .flatten()
            .map(|r| r.get("path"));
    ws.on_upgrade(move |socket| session(socket, path))
}

/// The user's shell, the way their terminal would start it.
fn shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) { "cmd.exe".into() } else { "/bin/sh".into() }
    })
}

async fn session(mut socket: WebSocket, path: Option<String>) {
    let Some(path) = path.filter(|p| std::path::Path::new(p).is_dir()) else {
        let _ = socket
            .send(Message::Text("this project's folder is not on disk\r\n".into()))
            .await;
        return;
    };

    let pty = match native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("could not open a pty: {e}\r\n").into()))
                .await;
            return;
        }
    };

    let mut cmd = CommandBuilder::new(shell());
    if !cfg!(windows) {
        // A login shell, so the PATH the user's own terminal would have —
        // node, cargo, pyenv — exists here too. The seconds it costs at open
        // are cheaper than every tool being "not found".
        cmd.arg("-l");
    }
    cmd.cwd(&path);
    cmd.env("TERM", "xterm-256color");
    // The same strip every engine child gets, from the same list.
    for key in aichip_shared::AICHIP_OWN_SECRETS {
        cmd.env_remove(key);
    }

    let mut child = match pty.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("could not start {}: {e}\r\n", shell()).into()))
                .await;
            return;
        }
    };
    // The slave is the child's side; holding our copy open would keep the
    // reader from ever seeing EOF after the shell exits.
    drop(pty.slave);

    let mut reader = match pty.master.try_clone_reader() {
        Ok(r) => r,
        Err(_) => {
            let _ = child.kill();
            return;
        }
    };
    let mut writer = match pty.master.take_writer() {
        Ok(w) => w,
        Err(_) => {
            let _ = child.kill();
            return;
        }
    };

    // PTY reads are blocking, so they live on a plain thread and hand chunks
    // to the async side over a channel. The thread ends at EOF — the shell
    // exiting — which closes the channel, which ends the socket loop.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            chunk = out_rx.recv() => match chunk {
                Some(bytes) => {
                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                // Shell exited. Say so in-band, then hang up.
                None => {
                    let _ = socket
                        .send(Message::Text("\r\n[session ended]\r\n".into()))
                        .await;
                    break;
                }
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Binary(bytes))) => {
                    if writer.write_all(&bytes).is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => {
                    if let Ok(ctl) = serde_json::from_str::<Control>(&text) {
                        if let Some(s) = ctl.resize {
                            let _ = pty.master.resize(PtySize {
                                rows: s.rows,
                                cols: s.cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            });
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
        }
    }

    // Whichever way the loop ended, the shell does not outlive the socket.
    let _ = child.kill();
    let _ = child.wait();
}
