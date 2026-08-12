//! How long aichip waits for you, and how it reaches you while it waits.
//!
//! One setting, because from a person's side they are one question. Every
//! notification aichip had was `new Notification()` from an open dashboard tab
//! — which solves being on the wrong tab, not being away from the computer.
//! Runs here take tens of minutes and cost real money, so "away from the
//! computer" is the normal case.
//!
//! Machine-level rather than per-project, deliberately. The command runs on
//! *this* machine's shell as *this* OS user; a per-project copy would imply a
//! distinction that does not exist. More importantly, project rows are created
//! from GitHub URLs and by agents building apps — putting a shell command on a
//! row an agent can write would hand an agent a shell.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use crate::db::Db;

/// Where the dashboard answers, for the link the hook hands you.
///
/// A global set once by `serve` rather than a parameter, because the two
/// callers that build a `Ctx` have no route to it: `DbGate` holds a `Db` and
/// nothing else, and adding a third constructor argument it otherwise has no
/// use for would be plumbing for one string. It is immutable after the
/// listener binds, which is the only situation a `OnceLock` is honest about.
static DASHBOARD_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Called by `serve` once it knows the address it actually bound.
pub fn set_dashboard_url(url: impl Into<String>) {
    let _ = DASHBOARD_URL.set(url.into());
}

/// The bound dashboard address, for callers building their own deep links
/// (a routine's delivery points at a chat thread or a report, shapes
/// [`link`] doesn't know).
pub fn dashboard_url() -> Option<&'static str> {
    DASHBOARD_URL.get().map(String::as_str)
}

/// A deep link to the thing that needs you.
///
/// Pure, so the shape is tested without a server. `ProjectPage` already reads
/// `?task=`, so the card opens rather than the board.
pub fn link(base: &str, project: Option<uuid::Uuid>, task: Option<uuid::Uuid>) -> String {
    let base = base.trim_end_matches('/');
    match (project, task) {
        (Some(p), Some(t)) => format!("{base}/projects/{p}?task={t}"),
        (Some(p), None) => format!("{base}/projects/{p}"),
        _ => base.to_string(),
    }
}

/// Longest a person can be asked to wait, and the ceiling on the setting.
pub const MAX_WINDOW_SECS: i64 = 7 * 24 * 3600;
/// How much longer the engine's own tool timeout is given than aichip's
/// window.
///
/// The two must not fire together. Whichever wins decides what the engine is
/// told, and only aichip's side can say "nobody answered, this is not a
/// refusal, stop" — the CLI's own timeout is a bare error the model reads as
/// a transient fault and retries around.
pub const CLI_GRACE_SECS: u64 = 120;

/// Longest the hook itself may take before it is killed.
const MAX_HOOK_SECS: i64 = 120;
/// Values handed to the hook are cut here. Titles come from GitHub issue
/// import among other places, and a notification is one line on a screen.
const MAX_VALUE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attention {
    /// Whether to run `command` at all.
    pub enabled: bool,
    /// A shell command line. Run through `sh -c` / `cmd /C`, so it is whatever
    /// the person would type: `notify-send "$AICHIP_TITLE" "$AICHIP_BODY"`,
    /// `curl -d "$AICHIP_BODY" ntfy.sh/mytopic`, a PowerShell toast.
    pub command: String,
    pub events: Vec<Event>,
    /// Seconds before a hook that has not exited is killed.
    pub hook_timeout_secs: i64,
    /// Seconds aichip waits for an answer before stopping the run. `0` means
    /// wait indefinitely — a legitimate choice, because a parked run lends its
    /// queue slot back and so costs almost nothing to keep.
    pub wait_secs: i64,
}

impl Default for Attention {
    fn default() -> Self {
        Self {
            enabled: false,
            command: String::new(),
            // The same four the browser notifier announces. One definition of
            // "blocked on a person", two ways of delivering it.
            events: vec![
                Event::Permission,
                Event::Plan,
                Event::RateLimited,
                Event::OverBudget,
                Event::Routine,
            ],
            hook_timeout_secs: 10,
            // Survives a night. Shorter re-creates the original bug in
            // miniature: asked at 6pm, dead by midnight.
            wait_secs: 24 * 3600,
        }
    }
}

impl Attention {
    /// How long the broker should wait, or `None` for indefinitely.
    pub fn window(&self) -> Option<Duration> {
        (self.wait_secs > 0).then(|| Duration::from_secs(self.wait_secs as u64))
    }

    fn wants(&self, event: Event) -> bool {
        self.enabled && !self.command.trim().is_empty() && self.events.contains(&event)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Permission,
    Plan,
    RateLimited,
    OverBudget,
    /// Off by default: it fires on every card, and a notification that always
    /// fires is one people learn to ignore.
    Finished,
    /// A routine delivered (or failed). On by default, unlike `Finished`:
    /// a routine fires on a schedule precisely because you are away, so
    /// "it ran, here's where the result is" is the half of the feature that
    /// happens off-screen.
    Routine,
}

impl Event {
    pub fn as_str(self) -> &'static str {
        match self {
            Event::Permission => "permission",
            Event::Plan => "plan",
            Event::RateLimited => "rate_limited",
            Event::OverBudget => "over_budget",
            Event::Finished => "finished",
            Event::Routine => "routine",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "permission" => Event::Permission,
            "plan" => Event::Plan,
            "rate_limited" => Event::RateLimited,
            "over_budget" => Event::OverBudget,
            "finished" => Event::Finished,
            "routine" => Event::Routine,
            _ => return None,
        })
    }
}

/// The variables a hook can read, for the settings screen.
///
/// `AICHIP_INPUT` is conspicuously not here, and must never be — see
/// [`payload`].
pub const ENV_NAMES: &[&str] = &[
    "AICHIP_EVENT",
    "AICHIP_TITLE",
    "AICHIP_BODY",
    "AICHIP_PROJECT",
    "AICHIP_CARD",
    "AICHIP_TOOL",
    "AICHIP_RUN_ID",
    "AICHIP_URL",
];

/// What the hook is told. Deliberately small.
#[derive(Debug, Default, Clone)]
pub struct Ctx {
    pub title: String,
    pub body: String,
    pub project: Option<String>,
    pub card: Option<String>,
    pub tool: Option<String>,
    pub run_id: Option<String>,
    pub url: Option<String>,
}

pub async fn load(db: &Db) -> Attention {
    let stored: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'attention'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    let d = Attention::default();
    let Some(v) = stored else { return d };
    Attention {
        enabled: v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(d.enabled),
        command: v
            .get("command")
            .and_then(|x| x.as_str())
            .unwrap_or(&d.command)
            .to_string(),
        events: v
            .get("events")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.as_str().and_then(Event::parse))
                    .collect()
            })
            .unwrap_or(d.events),
        // Clamped rather than trusted, the way `previews::limits` is: a hook
        // timeout of zero would kill every notification before it ran, and
        // there is no way back from that in the UI.
        hook_timeout_secs: v
            .get("hook_timeout_secs")
            .and_then(|x| x.as_i64())
            .unwrap_or(d.hook_timeout_secs)
            .clamp(1, MAX_HOOK_SECS),
        // 0 is meaningful here (wait forever), so the floor is 0, not 1.
        wait_secs: v
            .get("wait_secs")
            .and_then(|x| x.as_i64())
            .unwrap_or(d.wait_secs)
            .clamp(0, MAX_WINDOW_SECS),
    }
}

pub async fn save(db: &Db, next: Attention) -> anyhow::Result<Attention> {
    let next = Attention {
        hook_timeout_secs: next.hook_timeout_secs.clamp(1, MAX_HOOK_SECS),
        wait_secs: next.wait_secs.clamp(0, MAX_WINDOW_SECS),
        command: next.command.trim().to_string(),
        ..next
    };
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('attention', $1)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(serde_json::json!({
        "enabled": next.enabled,
        "command": next.command,
        "events": next.events.iter().map(|e| e.as_str()).collect::<Vec<_>>(),
        "hook_timeout_secs": next.hook_timeout_secs,
        "wait_secs": next.wait_secs,
    }))
    .execute(&db.pool)
    .await?;
    Ok(next)
}

/// Everything the hook is told about a run, looked up once.
///
/// A card's title and its project name, and nothing from the tool call beyond
/// its name — see [`payload`].
pub async fn ctx_for_run(db: &Db, run_id: uuid::Uuid, tool: Option<&str>) -> Ctx {
    let row = sqlx::query_as::<
        _,
        (Option<String>, Option<String>, Option<uuid::Uuid>, Option<uuid::Uuid>),
    >(
        "SELECT p.name, t.title, p.id, t.id
           FROM runs r
           LEFT JOIN tasks t ON t.id = r.task_id
           LEFT JOIN projects p ON p.id = COALESCE(t.project_id, (
               SELECT c.project_id FROM chats c WHERE c.id = r.chat_id))
          WHERE r.id = $1",
    )
    .bind(run_id)
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten();
    let (project, card, project_id, task_id) = row.unwrap_or((None, None, None, None));
    Ctx {
        title: match tool {
            Some(t) => format!("aichip: allow {t}?"),
            None => "aichip needs you".to_string(),
        },
        body: [project.as_deref(), card.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
        project,
        card,
        tool: tool.map(str::to_string),
        run_id: Some(run_id.to_string()),
        // Declared in `ENV_NAMES`, passed to the hook, and shown as a usable
        // chip in the settings panel — and until this line it was always
        // empty. A push that tells you something needs you and not where is
        // the half of the feature that does not work.
        url: DASHBOARD_URL
            .get()
            .map(|base| link(base, project_id, task_id)),
    }
}

/// What `MCP_TOOL_TIMEOUT` should be, in milliseconds, for a given window.
///
/// Pure and public so the orchestrator and the test agree on one function.
/// The old test re-derived the sum itself, which is why it passed while the
/// two halves were splitting apart in production.
pub fn cli_timeout_ms(window: Option<Duration>) -> String {
    let secs = window
        .map(|d| d.as_secs() + CLI_GRACE_SECS)
        .unwrap_or(MAX_WINDOW_SECS as u64);
    (secs * 1000).to_string()
}

/// Run the hook for one event. Never fails, never blocks the caller.
pub async fn fire(db: &Db, event: Event, ctx: Ctx) {
    let cfg = load(db).await;
    if !cfg.wants(event) {
        return;
    }
    tokio::spawn(async move { run_hook(&cfg, event, &ctx).await });
}

async fn run_hook(cfg: &Attention, event: Event, ctx: &Ctx) {
    let mut cmd = shell(&cfg.command);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        // Without this the timeout below would leave the child running and
        // only stop waiting for it, which is the shape of a hook that hangs
        // and is never noticed.
        .kill_on_drop(true);
    for (k, v) in payload(event, ctx) {
        cmd.env(k, v);
    }
    // A spawned child never inherits aichip's own secrets. Same rule the
    // engines and `gh` get; `env_clear()` is equally deliberately absent,
    // because the hook needs PATH, HOME, and on Linux the session bus.
    for key in aichip_shared::env_guard::AICHIP_OWN_SECRETS {
        cmd.env_remove(key);
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "attention hook could not start");
            return;
        }
    };
    let limit = Duration::from_secs(cfg.hook_timeout_secs as u64);
    match tokio::time::timeout(limit, child.wait_with_output()).await {
        Ok(Ok(out)) if out.status.success() => {}
        Ok(Ok(out)) => {
            let tail: String = String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(300)
                .collect();
            tracing::warn!(status = ?out.status.code(), stderr = %tail, "attention hook failed");
        }
        Ok(Err(e)) => tracing::warn!(error = %e, "attention hook errored"),
        Err(_) => tracing::warn!(secs = cfg.hook_timeout_secs, "attention hook timed out"),
    }
}

/// Build the shell invocation for this platform.
///
/// The configured text is one argument, always. It is never concatenated with
/// anything from a card, a project, or an engine — those travel as environment
/// variables, so a card titled `"; rm -rf ~ #"` is a string the hook may read
/// and never a fragment of the command line.
fn shell(command: &str) -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    }
    #[cfg(windows)]
    {
        // `cmd.exe` does not parse arguments the way Rust's default quoting
        // assumes, so a command containing quotes comes out mangled. `raw_arg`
        // passes the text through untouched, which is what `cmd /C` wants.
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.arg("/C");
        c.as_std_mut().raw_arg(command);
        c
    }
}

/// The environment the hook is given.
///
/// **The tool input is deliberately absent.** It carries file contents, diffs
/// and shell commands, and the hook is an arbitrary program that may log it or
/// post it anywhere. The dashboard *does* show the input — "Answering 'allow
/// Bash' without seeing the command is not a decision" — and the contrast is
/// the point: a loopback page a person is looking at, versus a command that may
/// forward. Whoever adds `AICHIP_INPUT` here is exfiltrating the repository.
fn payload(event: Event, ctx: &Ctx) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = vec![
        ("AICHIP_EVENT", event.as_str().to_string()),
        ("AICHIP_TITLE", ctx.title.clone()),
        ("AICHIP_BODY", ctx.body.clone()),
    ];
    let optional: [(&'static str, &Option<String>); 5] = [
        ("AICHIP_PROJECT", &ctx.project),
        ("AICHIP_CARD", &ctx.card),
        ("AICHIP_TOOL", &ctx.tool),
        ("AICHIP_RUN_ID", &ctx.run_id),
        ("AICHIP_URL", &ctx.url),
    ];
    for (k, v) in optional {
        out.push((k, v.clone().unwrap_or_default()));
    }
    for (_, v) in out.iter_mut() {
        *v = sanitize(v);
    }
    out
}

/// Strip control characters and cut to a line's worth.
///
/// Not decoration: a NUL in a value makes `Command::env` fail the spawn
/// outright, and terminal escapes land in whatever renders the notification.
/// Card titles come from GitHub issues, so neither is hypothetical.
fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_VALUE_CHARS)
        .collect()
}

/// A one-line map of what the hook will be given, for the settings screen.
pub fn sample_env(event: Event, ctx: &Ctx) -> HashMap<String, String> {
    payload(event, ctx)
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx {
            title: "Allow Bash?".into(),
            body: "windows11 · Build the launcher".into(),
            project: Some("windows11".into()),
            card: Some("Build the launcher".into()),
            tool: Some("Bash".into()),
            run_id: Some("abc".into()),
            url: Some("http://127.0.0.1:4820".into()),
        }
    }

    #[test]
    fn the_hook_never_receives_the_tool_input() {
        let env = sample_env(Event::Permission, &ctx());
        assert!(!env.contains_key("AICHIP_INPUT"));
        // And nothing else smuggles it either.
        for v in env.values() {
            assert!(!v.contains("rm -rf"), "{v}");
        }
    }

    #[test]
    fn the_command_is_never_built_by_concatenation() {
        let hostile = "; rm -rf ~ #";
        let cmd = shell("notify-send \"$AICHIP_TITLE\"");
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        // Exactly the configured text, as one argument.
        assert_eq!(args, vec!["-c", "notify-send \"$AICHIP_TITLE\""]);

        // A hostile card title reaches the hook as a value, never as syntax.
        let env = sample_env(
            Event::Permission,
            &Ctx {
                card: Some(hostile.into()),
                ..ctx()
            },
        );
        assert_eq!(env["AICHIP_CARD"], hostile);
    }

    #[test]
    fn the_documented_names_are_the_names_actually_passed() {
        // The settings screen shows ENV_NAMES. A name listed there that is
        // never set sends someone writing a hook against a variable that will
        // always be empty; one set but not listed is a variable nobody knows
        // about. Both directions, so a rename has to touch both.
        let passed: std::collections::HashSet<_> = payload(Event::Permission, &ctx())
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        let documented: std::collections::HashSet<_> = ENV_NAMES.iter().copied().collect();
        assert_eq!(passed, documented);
    }

    #[test]
    fn every_name_the_hook_gets_passes_the_env_guard() {
        // The invariant is that `is_auth_env` decides what counts as a secret,
        // never a hand-rolled list. If someone later adds AICHIP_RUN_TOKEN
        // here, this fails rather than leaking it.
        for (k, _) in payload(Event::Permission, &ctx()) {
            assert!(
                !aichip_shared::env_guard::is_auth_env(k),
                "{k} reads as an auth variable"
            );
        }
    }

    #[test]
    fn a_nul_or_an_escape_never_reaches_the_spawn() {
        let env = sample_env(
            Event::Permission,
            &Ctx {
                card: Some("bad\0title\u{1b}[31m".into()),
                ..ctx()
            },
        );
        assert_eq!(env["AICHIP_CARD"], "badtitle[31m");
    }

    #[test]
    fn a_long_title_is_cut_to_one_line() {
        let env = sample_env(
            Event::Permission,
            &Ctx {
                card: Some("x".repeat(5000)),
                ..ctx()
            },
        );
        assert_eq!(env["AICHIP_CARD"].chars().count(), MAX_VALUE_CHARS);
    }

    #[test]
    fn a_link_points_at_the_thing_that_needs_you() {
        let p = uuid::Uuid::nil();
        let t = uuid::Uuid::from_u128(1);
        assert_eq!(
            link("http://127.0.0.1:4820", Some(p), Some(t)),
            format!("http://127.0.0.1:4820/projects/{p}?task={t}")
        );
        assert_eq!(
            link("http://127.0.0.1:4820", Some(p), None),
            format!("http://127.0.0.1:4820/projects/{p}")
        );
        // Nothing to point at is the dashboard, not a broken path.
        assert_eq!(link("http://127.0.0.1:4820", None, None), "http://127.0.0.1:4820");
        // A trailing slash must not produce `//projects`.
        assert_eq!(
            link("http://127.0.0.1:4820/", Some(p), None),
            format!("http://127.0.0.1:4820/projects/{p}")
        );
    }

    #[test]
    fn the_url_the_settings_panel_advertises_is_not_always_empty() {
        // It was. `ENV_NAMES` listed it, `payload` passed it, the panel showed
        // it as a usable chip — and the only production constructor set it to
        // `None`, so every hook fired with AICHIP_URL="". A push that says
        // something needs you and cannot say where is half a feature.
        let env = sample_env(
            Event::Permission,
            &Ctx {
                url: Some(link("http://127.0.0.1:4820", Some(uuid::Uuid::nil()), None)),
                ..ctx()
            },
        );
        assert!(env["AICHIP_URL"].starts_with("http"), "{}", env["AICHIP_URL"]);
    }

    #[test]
    fn the_engine_always_waits_longer_than_aichip_does() {
        // If these ever meet, the CLI's timeout wins the race and the engine
        // is told a bare "tool timed out" — which it treats as a transient
        // fault and retries around, spending money on an edit nobody approved.
        for secs in [1, 60, 3600, 24 * 3600, MAX_WINDOW_SECS] {
            let cfg = Attention {
                wait_secs: secs,
                ..Default::default()
            };
            let window = cfg.window().unwrap().as_secs();
            // Through `cli_timeout_ms` itself, not a re-derivation of it. The
            // old version computed the sum in the test, which is why it stayed
            // green while the two halves were splitting in production.
            let engine_ms: u64 = cli_timeout_ms(cfg.window()).parse().unwrap();
            assert!(
                engine_ms > window * 1000,
                "engine {engine_ms}ms must outlast the {window}s window"
            );
            assert!(CLI_GRACE_SECS >= 60, "the margin must survive a slow answer");
        }
    }

    #[test]
    fn waiting_forever_is_expressible_and_the_default_is_not_forever() {
        assert_eq!(
            Attention {
                wait_secs: 0,
                ..Default::default()
            }
            .window(),
            None
        );
        assert_eq!(
            Attention::default().window(),
            Some(Duration::from_secs(24 * 3600))
        );
    }

    #[test]
    fn a_hook_with_no_command_never_fires() {
        // Enabled with an empty command is a half-filled form, not a request
        // to spawn an empty shell on every prompt.
        let cfg = Attention {
            enabled: true,
            command: "   ".into(),
            ..Default::default()
        };
        assert!(!cfg.wants(Event::Permission));
    }

    #[test]
    fn finished_is_off_unless_asked_for() {
        assert!(!Attention::default().events.contains(&Event::Finished));
    }

    #[test]
    fn routine_deliveries_are_on_by_default() {
        // The opposite default from Finished, on purpose: a routine fires on
        // a schedule precisely because nobody is watching.
        assert!(Attention::default().events.contains(&Event::Routine));
    }

    #[test]
    fn an_event_name_survives_a_round_trip() {
        for e in [
            Event::Permission,
            Event::Plan,
            Event::RateLimited,
            Event::OverBudget,
            Event::Finished,
            Event::Routine,
        ] {
            assert_eq!(Event::parse(e.as_str()), Some(e));
        }
        assert_eq!(Event::parse("nonsense"), None);
    }
}
