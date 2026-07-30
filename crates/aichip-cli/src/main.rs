use aichip_core::runs::permissions::PermissionBroker;
use aichip_core::{Db, EventBus, Orchestrator, WorktreeManager};
use aichip_engines::claude::ClaudeEngine;
use aichip_engines::opencode::OpenCodeEngine;
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

/// The engines aichip knows how to drive, in the order they're offered.
///
/// One list, used by both `serve` and `doctor`, so adding a third adapter
/// never means remembering to edit the doctor separately.
fn real_engines() -> Vec<Arc<dyn Engine>> {
    vec![
        Arc::new(ClaudeEngine::default()) as Arc<dyn Engine>,
        Arc::new(OpenCodeEngine::default()) as Arc<dyn Engine>,
    ]
}

/// Provider names and auth *type* — never a credential.
fn describe_providers(info: &aichip_engines::EngineInfo) -> String {
    let s = info
        .providers
        .iter()
        .map(|p| format!("{} ({})", p.name, p.auth))
        .collect::<Vec<_>>()
        .join(", ");
    if s.is_empty() {
        "—".into()
    } else {
        s
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
    // Before anything serves a request: a half-migrated wiki answers searches
    // wrong for some pages and says nothing about why.
    if let Err(e) = aichip_core::kb::backfill::run(&db).await {
        tracing::error!(error = %e, "knowledge-base migration failed; some pages may not be searchable");
    }

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
    // Register only what is actually installed, so an engine that isn't
    // present is simply *not offered* rather than accepted and then failing
    // at spawn time.
    for engine in real_engines() {
        let id = engine.id();
        match orchestrator.register_if_available(engine).await {
            Some(info) => tracing::info!(
                id,
                version = %info.version,
                providers = %describe_providers(&info),
                "engine available"
            ),
            None => tracing::info!(id, "engine not found on PATH; it will not be offered"),
        }
    }
    orchestrator.register_engine(Arc::new(MockEngine::demo()) as Arc<dyn Engine>);
    let orchestrator = Arc::new(orchestrator);

    // Model routing is the user's choice, so it has to be in place before the
    // first run is claimed rather than applied on the one after.
    orchestrator.load_tier_mapping().await?;

    let orphans = orchestrator.recover_orphans().await?;
    if orphans > 0 {
        tracing::warn!(orphans, "marked orphaned runs from previous session as failed");
    }
    tokio::spawn(orchestrator.clone().run_loop());
    tokio::spawn(aichip_core::Scheduler::new(db.clone(), orchestrator.clone()).run_loop());
    tokio::spawn(sweep_attachments(db.clone()));

    // Object storage, when it's configured. The bucket is created on boot so
    // a fresh MinIO needs no manual setup step; a failure here is logged and
    // the feature simply stays off rather than taking the server down.
    let storage = match aichip_core::storage::Storage::from_env() {
        Some(s) => match s.ensure_bucket().await {
            Ok(()) => {
                tracing::info!(bucket = s.bucket(), "object storage ready");
                Some(s)
            }
            Err(e) => {
                tracing::warn!(error = %e, "object storage unreachable; attachments disabled");
                None
            }
        },
        None => {
            tracing::info!("object storage not configured; knowledge-base attachments disabled");
            None
        }
    };

    let state = aichip_server::AppState {
        db,
        bus: bus.clone(),
        orchestrator,
        permissions: PermissionBroker::new(bus),
        storage,
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
    // inspecting their config or credential files. That is exactly what
    // `Engine::detect` does, which is why this loops the registry rather than
    // hand-coding a `--version` call per engine.
    let mut ok = true;

    match run_version("git", &["--version"]).await {
        Some(v) => println!("✓ git: {v}"),
        None => {
            println!("✗ git: not found on PATH");
            ok = false;
        }
    }

    // GitHub is optional — everything works without it — so a missing `gh` is
    // reported with a dot rather than a cross, the same way an uninstalled
    // engine is. An *expired* login is different: `gh` is right there and every
    // command it runs will fail, so it says which account and why.
    match aichip_core::github::detect().await {
        None => println!("· gh: not installed — GitHub features won't be offered"),
        Some(info) if info.usable() => {
            let who = info
                .active()
                .map(|a| format!(" — {} on {}", a.login, a.host))
                .unwrap_or_default();
            println!("✓ gh: {}{who}", info.version);
        }
        Some(info) => {
            // `!` rather than `✗`, and `ok` is deliberately left alone: aichip
            // is completely usable without GitHub, so this must not fail a
            // setup check or contradict the "All good" at the end. It is a
            // thing to know, not a thing that is broken.
            println!("! gh: {} — not logged in", info.version);
            for account in &info.accounts {
                if let Some(problem) = &account.problem {
                    println!("    {} on {}: {problem}", account.login, account.host);
                }
            }
            println!("    GitHub features stay hidden until: gh auth login");
        }
    }

    let mut found = 0;
    for engine in real_engines() {
        match engine.detect().await {
            Some(info) => {
                found += 1;
                println!("✓ {}: {}", engine.label(), info.version);
                let providers = describe_providers(&info);
                if providers != "—" {
                    println!("  providers: {providers}");
                }
                let caps = engine.capabilities();
                if !caps.interactive_permissions {
                    println!("  note: can't ask permission mid-run — use Auto-edit or Don't-ask");
                }
                if !caps.structured_rate_limit {
                    println!("  note: no rate-limit signal, so the queue can't back off for it");
                }
            }
            None => println!("· {}: not installed — it won't be offered", engine.label()),
        }
    }

    if found == 0 {
        println!("\n✗ no agent CLI found. Install at least one:");
        println!("    claude   → https://code.claude.com");
        println!("    opencode → https://opencode.ai");
        ok = false;
    } else {
        println!("\n  (login is verified on first run; if runs fail immediately, start the");
        println!("   CLI interactively once and log in)");
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
