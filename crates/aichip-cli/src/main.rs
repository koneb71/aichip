use aichip_core::runs::gate::{DbGate, DbWindow};
use aichip_core::runs::permissions::PermissionBroker;
use aichip_core::{Db, EventBus, Orchestrator, WorktreeManager};
use aichip_engines::claude::ClaudeEngine;
use aichip_engines::codex::CodexEngine;
use aichip_engines::local::LocalEngine;
use aichip_engines::mock::MockEngine;
use aichip_engines::opencode::OpenCodeEngine;
use aichip_engines::Engine;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "aichip",
    about = "Local-first multi-agent workflow platform — no API keys"
)]
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

/// Where the local runtimes listen, when somebody has said.
///
/// `None` is not "the default" — see `aichip_core::local_models::configured`.
#[derive(Default)]
struct LocalHosts {
    ollama: Option<String>,
    lmstudio: Option<String>,
}

/// The engines aichip knows how to drive, in the order they're offered.
///
/// One list, used by both `serve` and `doctor`, so adding another adapter
/// never means remembering to edit the doctor separately.
///
/// The last two are the same OpenCode binary pointed at a model on this
/// machine — see `aichip_engines::local` for why that is an engine and not a
/// setting. `doctor` runs without a database, so it passes no addresses and
/// gets the stock ports; that costs nothing, because the only thing an
/// address changes is where the probe looks.
fn real_engines(local: LocalHosts) -> Vec<Arc<dyn Engine>> {
    vec![
        Arc::new(ClaudeEngine::default()) as Arc<dyn Engine>,
        Arc::new(OpenCodeEngine::default()) as Arc<dyn Engine>,
        Arc::new(CodexEngine::default()) as Arc<dyn Engine>,
        Arc::new(LocalEngine::ollama(local.ollama)) as Arc<dyn Engine>,
        Arc::new(LocalEngine::lmstudio(local.lmstudio)) as Arc<dyn Engine>,
    ]
}

/// Where to get an engine this machine hasn't got.
///
/// Beside the "not installed" line rather than in a wall of links at the end,
/// because the person reading it has just been told about one specific thing.
fn install_hint(id: &str) -> Option<&'static str> {
    match id {
        "claude-code" => Some("https://code.claude.com"),
        "opencode" => Some("https://opencode.ai"),
        "codex" => Some("npm i -g @openai/codex — https://developers.openai.com/codex/cli"),
        "ollama" => Some("https://ollama.com — needs OpenCode too, to drive it"),
        "lmstudio" => Some("https://lmstudio.ai — needs OpenCode too, to drive it"),
        _ => None,
    }
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
    // Decided first, before a database is started or a port is claimed: a
    // refusal that arrives ten seconds in has already cost something, and the
    // thing it refuses is a configuration mistake somebody wants to hear about
    // immediately.
    // Loopback by default: this is a local-first app with no authentication of
    // any kind, and that is only affordable while the only possible caller is
    // this machine. Binding anywhere else spends that, so it has to be said out
    // loud — see `aichip_server::exposure`, which also explains why refusing
    // non-loopback *callers* would not work.
    let bind: std::net::IpAddr = std::env::var("AICHIP_BIND")
        .ok()
        .and_then(|b| b.parse().ok())
        .unwrap_or(std::net::IpAddr::from([127, 0, 0, 1]));
    let acknowledged = std::env::var(aichip_server::TRUST_NETWORK)
        .is_ok_and(|v| !v.trim().is_empty() && v.trim() != "0");
    match aichip_server::exposure(bind, acknowledged) {
        aichip_server::Exposure::Local => {}
        aichip_server::Exposure::Network => {
            tracing::warn!(
                %bind,
                "aichip is reachable from your network and has no authentication — \
                 anyone who can reach this port can read transcripts, browse files \
                 and start agents"
            );
        }
        aichip_server::Exposure::Unacknowledged => {
            // Refused rather than warned: a warning in a log is not read by the
            // person who copied a line from somewhere and moved on.
            anyhow::bail!(aichip_server::unacknowledged_message(bind));
        }
    }

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
    // Read before the engines are built rather than threaded to each spawn
    // site: a local engine knows where its own runtime lives, which is the
    // only place that fact was ever needed.
    let (ollama, lmstudio) = aichip_core::local_models::configured(&db).await;
    // Register only what is actually installed, so an engine that isn't
    // present is simply *not offered* rather than accepted and then failing
    // at spawn time.
    for engine in real_engines(LocalHosts { ollama, lmstudio }) {
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
    orchestrator.load_tier_efforts().await?;
    // Previews do not survive a restart, and the containers from the last one
    // are still holding their ports. Settled against Docker rather than the
    // table, so a container no row claims is swept rather than orphaned.
    if let Err(e) = aichip_core::previews::reconcile(&db).await {
        tracing::warn!(error=%e, "could not reconcile previews with docker");
    }

    // Worktrees nothing can reach any more: a bake-off variant whose run
    // cascaded away with its card, a workflow fan-out directory whose id was
    // never written down, an uninstalled app's leftovers. Same reason previews
    // and attachments get a sweep — Postgres can drop a row but not a
    // directory, and these are the largest thing aichip puts on disk.
    match aichip_core::worktrees::sweep::reconcile(&db, &orchestrator.worktrees).await {
        Ok(s) if s.worktrees > 0 || s.dead_projects > 0 => tracing::info!(
            worktrees = s.worktrees,
            mb = s.bytes / 1_000_000,
            dead_projects = s.dead_projects,
            "reclaimed worktrees nothing was using"
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "could not sweep worktrees"),
    }

    // One small file per run and per preview, in three directories, none of
    // which anything had ever cleaned. Individually kilobytes; unbounded in
    // count, and every one keyed by an id the database can be asked about.
    match aichip_core::leftovers::sweep(&db).await {
        Ok(s) if s.files > 0 => tracing::info!(
            files = s.files,
            kb = s.bytes / 1024,
            "removed per-run files whose run is long finished"
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "could not sweep per-run files"),
    }

    let orphans = orchestrator.recover_orphans().await?;
    if orphans > 0 {
        tracing::warn!(
            orphans,
            "marked orphaned runs from previous session as failed"
        );
    }
    tokio::spawn(orchestrator.clone().run_loop());
    tokio::spawn(aichip_core::Scheduler::new(db.clone(), orchestrator.clone()).run_loop());
    tokio::spawn(sweep_attachments(db.clone()));
    // Previews nobody is looking at stop themselves, keeping their images so
    // coming back costs seconds rather than a rebuild.
    tokio::spawn(aichip_core::previews::idle_loop(db.clone()));

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

    // Built before the state literal, which moves `db` on its first line.
    // The broker needs a database of its own to park and unpark runs, and the
    // orchestrator's slots so a run waiting on a person stops occupying one.
    let permissions = {
        let cancel_orchestrator = orchestrator.clone();
        PermissionBroker::new(
            bus.clone(),
            Arc::new(DbGate::new(db.clone(), move |run_id| {
                cancel_orchestrator.cancel(run_id);
            })),
            orchestrator.slots(),
            // Asked per prompt, so it can never disagree with the engine
            // timeout the orchestrator derives from the same setting.
            Arc::new(DbWindow(db.clone())),
        )
    };

    let state = aichip_server::AppState {
        db,
        bus: bus.clone(),
        orchestrator,
        permissions,
        storage,
        file_writes: Default::default(),
    };
    let app = aichip_server::app(state);

    let addr = std::net::SocketAddr::new(bind, port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    // The address it is actually on, not an assumed loopback one.
    tracing::info!("aichip dashboard: http://{}:{port}", displayable(bind));
    // The address it actually bound, not the hardcoded loopback the MCP base
    // uses — a spawned CLI is on this machine, but the person reading the push
    // on their phone is not, and a loopback link can never answer them.
    aichip_core::attention::set_dashboard_url(format!("http://{}:{port}", displayable(bind)));

    if !headless {
        let _ = tokio::process::Command::new("open")
            .arg(format!("http://127.0.0.1:{port}"))
            .spawn();
    }

    axum::serve(listener, app).await?;
    Ok(())
}

/// How to spell a bind address in a URL somebody can click.
///
/// `0.0.0.0` is not an address you connect to, so printing it as a link sends
/// people somewhere that does not answer; from this machine the answer is
/// loopback either way.
fn displayable(bind: std::net::IpAddr) -> String {
    if bind.is_unspecified() {
        "127.0.0.1".to_string()
    } else if bind.is_ipv6() {
        format!("[{bind}]")
    } else {
        bind.to_string()
    }
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
    for engine in real_engines(LocalHosts::default()) {
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
            None => {
                print!("· {}: not installed — it won't be offered", engine.label());
                match install_hint(engine.id()) {
                    Some(where_from) => println!("\n    {where_from}"),
                    None => println!(),
                }
            }
        }
    }

    // A local runtime is the one case where "not installed" can be wrong: the
    // app is right there and its server is switched off, or OpenCode — which
    // is what actually drives it — is the piece that's missing. `detect` can
    // only say yes or no, so the difference is spelled out here.
    for hint in aichip_engines::local::hints("opencode").await {
        println!("\n! {hint}");
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

#[cfg(test)]
mod tests {
    use super::displayable;

    #[test]
    fn an_unspecified_bind_is_shown_as_somewhere_you_can_actually_click() {
        // `http://0.0.0.0:4820` is not a place; from this machine the answer is
        // loopback whatever it bound to.
        assert_eq!(displayable("0.0.0.0".parse().unwrap()), "127.0.0.1");
        assert_eq!(displayable("::".parse().unwrap()), "127.0.0.1");
        // A specific address is itself, and IPv6 needs its brackets in a URL.
        assert_eq!(displayable("192.168.1.5".parse().unwrap()), "192.168.1.5");
        assert_eq!(displayable("::1".parse().unwrap()), "[::1]");
    }
}
