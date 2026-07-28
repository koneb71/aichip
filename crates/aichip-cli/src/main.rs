use aichip_core::runs::permissions::PermissionBroker;
use aichip_core::{Db, EventBus, Orchestrator, WorktreeManager};
use aichip_engines::claude::ClaudeEngine;
use aichip_engines::mock::MockEngine;
use aichip_engines::Engine;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "aichip", about = "Local-first multi-agent workflow platform — no API keys")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start the aichip server and open the dashboard (default).
    Serve {
        #[arg(long, default_value_t = 4820)]
        port: u16,
        /// Don't open the browser.
        #[arg(long)]
        headless: bool,
    },
    /// Check that required tools (git, claude CLI) are installed and usable.
    Doctor,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command.unwrap_or(Cmd::Serve {
        port: 4820,
        headless: false,
    }) {
        Cmd::Serve { port, headless } => serve(port, headless).await,
        Cmd::Doctor => doctor().await,
    }
}

/// Reclaim attachment bytes: uploads the user abandoned by closing a composer,
/// and directories whose row was cascade-deleted with the task or chat.
///
/// Never fatal — a failure here costs disk space, not correctness, so it logs
/// and keeps going rather than taking the server down.
async fn sweep_attachments(db: aichip_core::db::Db) {
    loop {
        match aichip_core::runs::attachments::sweep_abandoned(&db).await {
            Ok(n) if n > 0 => tracing::info!(removed = n, "swept abandoned attachments"),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "attachment sweep failed"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

fn aichip_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aichip")
}

async fn serve(port: u16, headless: bool) -> anyhow::Result<()> {
    let home = aichip_home();
    tokio::fs::create_dir_all(&home).await?;

    // Database: the user's own DATABASE_URL wins; otherwise boot a private
    // embedded Postgres with data under ~/.aichip/pgdata.
    let (database_url, _embedded) = match std::env::var("DATABASE_URL") {
        Ok(url) => (url, None),
        Err(_) => {
            // Retry: a previous instance's postgres may still be shutting
            // down for a few seconds after the old server exits.
            let mut attempt = 0;
            let pg = loop {
                match start_embedded_postgres(&home).await {
                    Ok(pg) => break pg,
                    Err(e) if attempt < 4 => {
                        attempt += 1;
                        tracing::warn!(error=%e, attempt, "embedded postgres not ready; retrying");
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    }
                    Err(e) => return Err(e),
                }
            };
            (pg.settings().url("aichip"), Some(pg))
        }
    };

    let db = Db::connect(&database_url).await?;
    let bus = EventBus::new();
    let worktrees = Arc::new(WorktreeManager::new(WorktreeManager::default_root()));
    let mcp_base = format!("http://127.0.0.1:{port}");

    let mut orchestrator = Orchestrator::new(
        db.clone(),
        bus.clone(),
        worktrees,
        max_concurrent(),
        Some(mcp_base),
    );
    orchestrator.register_engine(Arc::new(ClaudeEngine::default()) as Arc<dyn Engine>);
    orchestrator.register_engine(Arc::new(MockEngine::demo()) as Arc<dyn Engine>);
    let orchestrator = Arc::new(orchestrator);

    let orphans = orchestrator.recover_orphans().await?;
    if orphans > 0 {
        tracing::warn!(orphans, "marked orphaned runs from previous session as failed");
    }
    tokio::spawn(orchestrator.clone().run_loop());
    tokio::spawn(aichip_core::Scheduler::new(db.clone(), orchestrator.clone()).run_loop());
    tokio::spawn(sweep_attachments(db.clone()));

    let state = aichip_server::AppState {
        db,
        bus: bus.clone(),
        orchestrator,
        permissions: PermissionBroker::new(bus),
    };
    let app = aichip_server::app(state);

    // Loopback by default: this is a local-first app and binding wide on a
    // laptop would expose it to the network. A container overrides it,
    // because there the port is only reachable through an explicit mapping
    // — and the Host-header allowlist still guards it either way.
    let bind: std::net::IpAddr = std::env::var("AICHIP_BIND")
        .ok()
        .and_then(|b| b.parse().ok())
        .unwrap_or(std::net::IpAddr::from([127, 0, 0, 1]));
    let addr = std::net::SocketAddr::new(bind, port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("aichip dashboard: http://127.0.0.1:{port}");

    if !headless {
        let _ = tokio::process::Command::new("open")
            .arg(format!("http://127.0.0.1:{port}"))
            .spawn();
    }

    axum::serve(listener, app).await?;
    Ok(())
}

async fn start_embedded_postgres(
    home: &std::path::Path,
) -> anyhow::Result<postgresql_embedded::PostgreSQL> {
    use postgresql_embedded::{PostgreSQL, Settings};

    // The cluster is initialized with a password on first boot; every later
    // boot must present the same one, so persist it beside the data dir.
    let password_file = home.join("pg_password");
    let password = match tokio::fs::read_to_string(&password_file).await {
        Ok(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => {
            use rand::distr::{Alphanumeric, SampleString};
            let p = Alphanumeric.sample_string(&mut rand::rng(), 32);
            tokio::fs::write(&password_file, &p).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &password_file,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
            p
        }
    };

    let settings = Settings {
        data_dir: home.join("pgdata"),
        temporary: false,
        password,
        ..Settings::default()
    };
    let mut pg = PostgreSQL::new(settings);
    pg.setup().await?;
    pg.start().await?;
    if !pg.database_exists("aichip").await? {
        pg.create_database("aichip").await?;
    }
    tracing::info!(port = pg.settings().port, "embedded postgres up");
    Ok(pg)
}

fn max_concurrent() -> usize {
    std::env::var("AICHIP_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2)
}

async fn doctor() -> anyhow::Result<()> {
    // Compliance invariant: we probe tools by RUNNING them, never by
    // inspecting their config or credential files.
    let mut ok = true;

    match run_version("git", &["--version"]).await {
        Some(v) => println!("✓ git: {v}"),
        None => {
            println!("✗ git: not found on PATH");
            ok = false;
        }
    }

    match run_version("claude", &["--version"]).await {
        Some(v) => {
            println!("✓ claude CLI: {v}");
            println!("  (login status is verified on first run; if runs fail immediately,");
            println!("   run `claude` interactively once and log in)");
        }
        None => {
            println!("✗ claude CLI: not found on PATH — install from https://code.claude.com");
            ok = false;
        }
    }

    if ok {
        println!("\nAll good. Start with: aichip serve");
    } else {
        std::process::exit(1);
    }
    Ok(())
}

async fn run_version(bin: &str, args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new(bin)
        .args(args)
        .output()
        .await
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
