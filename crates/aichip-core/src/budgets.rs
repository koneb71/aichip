//! Budgets: what a scope may spend in a window, and what happens when it has.
//!
//! The old budget was one dollar figure a day for the whole machine, checked
//! by not claiming from the queue. It could not say "this project gets $20 a
//! week" or "the nightly routine gets 200k tokens"; an engine that reports no
//! price counted as free however much it ran; and a person clicking Start
//! over budget saw the card queue and sit there until midnight.
//!
//! A [`Policy`] covers a scope — the machine, a workspace, a project, an
//! agent, a team or a routine — over a calendar window, and caps any of
//! dollars, output tokens and runs. Tokens are how an engine that never says
//! what a run cost (`Capabilities::reports_cost == false`) is counted at all.
//!
//! Where it bites, from the cheapest moment to the last:
//!
//! - **at the click** — [`check`] refuses new work for a spent scope with a
//!   sentence naming the policy (a 409), rather than queueing it to wait;
//! - **at the queue** — `claim_next` holds a queued run whose scope is spent
//!   until the window turns, and claims the next one instead, so one spent
//!   project does not stop the others;
//! - **between steps** — a team or a workflow stops starting new work;
//! - **mid-run** — on a `stop` policy, a run that crosses a token cap is
//!   interrupted. Dollars cannot stop a run midway: no engine says what a run
//!   cost until it ends.
//!
//! A read that fails lets work through, as the old gate did — a database
//! hiccup must not stop every run on the machine — and says so in the log.

use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::db::Db;
use crate::spend::PROJECT_JOIN;

/// What a policy covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Machine,
    Workspace,
    Project,
    Agent,
    Team,
    Routine,
}

impl ScopeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Machine => "machine",
            Self::Workspace => "workspace",
            Self::Project => "project",
            Self::Agent => "agent",
            Self::Team => "team",
            Self::Routine => "routine",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "machine" => Self::Machine,
            "workspace" => Self::Workspace,
            "project" => Self::Project,
            "agent" => Self::Agent,
            "team" => Self::Team,
            "routine" => Self::Routine,
            _ => return None,
        })
    }
}

/// The calendar window a policy resets on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    Day,
    Week,
    Month,
}

impl WindowKind {
    /// Also the `date_trunc` field name, which is why it is an enum and never
    /// text from a request.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "day" => Self::Day,
            "week" => Self::Week,
            "month" => Self::Month,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub id: Uuid,
    pub name: String,
    pub scope_kind: ScopeKind,
    pub scope_id: Option<Uuid>,
    pub window_kind: WindowKind,
    pub cap_usd: Option<f64>,
    pub cap_output_tokens: Option<i64>,
    pub cap_runs: Option<i32>,
    pub warn_percent: i32,
    /// `true` for `on_exceed = 'stop'`: a run in flight is stopped at a token
    /// cap, not only new work held.
    pub stops_in_flight: bool,
    pub confirm_above_usd: Option<f64>,
    pub enabled: bool,
}

const POLICY_COLUMNS: &str =
    "id, name, scope_kind, scope_id, window_kind, cap_usd, cap_output_tokens,
     cap_runs, warn_percent, on_exceed, confirm_above_usd, enabled";

fn policy_from(r: &sqlx::postgres::PgRow) -> Option<Policy> {
    Some(Policy {
        id: r.get("id"),
        name: r.get("name"),
        scope_kind: ScopeKind::parse(&r.get::<String, _>("scope_kind"))?,
        scope_id: r.get("scope_id"),
        window_kind: WindowKind::parse(&r.get::<String, _>("window_kind"))?,
        cap_usd: r.get("cap_usd"),
        cap_output_tokens: r.get("cap_output_tokens"),
        cap_runs: r.get("cap_runs"),
        warn_percent: r.get("warn_percent"),
        stops_in_flight: r.get::<String, _>("on_exceed") == "stop",
        confirm_above_usd: r.get("confirm_above_usd"),
        enabled: r.get("enabled"),
    })
}

/// What a scope has spent in a window, net of overrides.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub usd: f64,
    pub output_tokens: i64,
    pub runs: i64,
}

/// Which cap a verdict is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    Usd,
    Tokens,
    Runs,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Verdict {
    Open,
    /// Past the warning line on at least one cap; `percent` is the highest.
    Warn {
        percent: u32,
    },
    /// At or past a cap. Another run would go over, so none starts.
    Exceeded {
        cap: Cap,
    },
}

/// Where a policy stands. Pure: every rule about caps is here, tested
/// without a database.
///
/// "At the cap" counts as exceeded, because the question is always "may one
/// more start?" — and at $40.00 of $40 the answer is no.
pub fn evaluate(policy: &Policy, used: &Usage) -> Verdict {
    let caps = [
        (Cap::Usd, policy.cap_usd, used.usd),
        (
            Cap::Tokens,
            policy.cap_output_tokens.map(|c| c as f64),
            used.output_tokens as f64,
        ),
        (Cap::Runs, policy.cap_runs.map(f64::from), used.runs as f64),
    ];
    let mut highest: Option<f64> = None;
    for (cap, limit, used) in caps {
        let Some(limit) = limit.filter(|l| *l > 0.0) else {
            continue;
        };
        if used >= limit {
            return Verdict::Exceeded { cap };
        }
        let fraction = used / limit;
        highest = Some(highest.map_or(fraction, |h| h.max(fraction)));
    }
    match highest {
        Some(f) if f * 100.0 >= f64::from(policy.warn_percent) => Verdict::Warn {
            percent: (f * 100.0).floor() as u32,
        },
        _ => Verdict::Open,
    }
}

/// "$41.20 of $40", "182k of 150k output tokens", "12 of 12 runs".
pub fn describe(policy: &Policy, used: &Usage, cap: Cap) -> String {
    match cap {
        Cap::Usd => format!("${:.2} of ${:.2}", used.usd, policy.cap_usd.unwrap_or(0.0)),
        Cap::Tokens => format!(
            "{} of {} output tokens",
            compact(used.output_tokens),
            compact(policy.cap_output_tokens.unwrap_or(0))
        ),
        Cap::Runs => format!("{} of {} runs", used.runs, policy.cap_runs.unwrap_or(0)),
    }
}

fn compact(n: i64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 10_000 => format!("{}k", n / 1000),
        n => n.to_string(),
    }
}

/// A scope is spent. Its sentence is what a person sees on the click that
/// was refused, or on the card a stopped run belonged to.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
#[error("budget \u{201c}{policy}\u{201d} is spent for this {window} ({detail}) — it resets {resets}; raise it or override it in Activity")]
pub struct OverBudget {
    pub policy: String,
    pub policy_id: Uuid,
    pub window: &'static str,
    pub detail: String,
    pub resets: String,
    pub resets_at: DateTime<Utc>,
}

/// What a piece of work belongs to — every scope a policy might name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scope {
    pub workspace: Option<Uuid>,
    pub project: Option<Uuid>,
    pub agent: Option<Uuid>,
    pub team: Option<Uuid>,
    pub routine: Option<Uuid>,
}

/// The scope of an existing run: its card's, or its own.
pub async fn scope_of_run(db: &Db, run_id: Uuid) -> anyhow::Result<Scope> {
    let row = sqlx::query(&format!(
        "SELECT COALESCE(p.workspace_id, c.workspace_id) AS workspace, p.id AS project,
                COALESCE(r.agent_id, t.agent_id) AS agent, COALESCE(r.team_id, t.team_id) AS team,
                (SELECT rr.routine_id FROM routine_runs rr
                  WHERE rr.run_id = r.id OR (r.task_id IS NOT NULL AND rr.task_id = r.task_id)
                  ORDER BY rr.fired_at DESC LIMIT 1) AS routine
           FROM runs r {PROJECT_JOIN}
          WHERE r.id = $1"
    ))
    .bind(run_id)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(scope_from).unwrap_or_default())
}

/// The scope a card's next run would have.
pub async fn scope_of_task(db: &Db, task_id: Uuid) -> anyhow::Result<Scope> {
    let row = sqlx::query(
        "SELECT p.workspace_id AS workspace, p.id AS project, t.agent_id AS agent,
                t.team_id AS team,
                (SELECT rr.routine_id FROM routine_runs rr WHERE rr.task_id = t.id
                  ORDER BY rr.fired_at DESC LIMIT 1) AS routine
           FROM tasks t JOIN projects p ON p.id = t.project_id
          WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(scope_from).unwrap_or_default())
}

fn scope_from(r: sqlx::postgres::PgRow) -> Scope {
    Scope {
        workspace: r.get("workspace"),
        project: r.get("project"),
        agent: r.get("agent"),
        team: r.get("team"),
        routine: r.get("routine"),
    }
}

/// Which enabled policies cover this scope. The machine's are included only
/// when asked — `claim_next` checks those once for the whole queue.
pub async fn policies_for(
    db: &Db,
    scope: &Scope,
    with_machine: bool,
) -> anyhow::Result<Vec<Policy>> {
    let rows = sqlx::query(&format!(
        "SELECT {POLICY_COLUMNS} FROM budget_policies
          WHERE enabled AND (
                ($1 AND scope_kind = 'machine')
             OR (scope_kind = 'workspace' AND scope_id = $2)
             OR (scope_kind = 'project'   AND scope_id = $3)
             OR (scope_kind = 'agent'     AND scope_id = $4)
             OR (scope_kind = 'team'      AND scope_id = $5)
             OR (scope_kind = 'routine'   AND scope_id = $6))
          ORDER BY created_at"
    ))
    .bind(with_machine)
    .bind(scope.workspace)
    .bind(scope.project)
    .bind(scope.agent)
    .bind(scope.team)
    .bind(scope.routine)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows.iter().filter_map(policy_from).collect())
}

pub async fn get(db: &Db, id: Uuid) -> anyhow::Result<Option<Policy>> {
    let row = sqlx::query(&format!(
        "SELECT {POLICY_COLUMNS} FROM budget_policies WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.as_ref().and_then(policy_from))
}

pub async fn list(db: &Db) -> anyhow::Result<Vec<Policy>> {
    let rows = sqlx::query(&format!(
        "SELECT {POLICY_COLUMNS} FROM budget_policies ORDER BY created_at"
    ))
    .fetch_all(&db.pool)
    .await?;
    Ok(rows.iter().filter_map(policy_from).collect())
}

/// Is any policy switched on? The queue's fast path: with none, claiming is
/// exactly what it was before budgets existed.
pub async fn any_enabled(db: &Db) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM budget_policies WHERE enabled)")
        .fetch_one(&db.pool)
        .await
        .unwrap_or(false)
}

/// The window a policy is in now: where it started, and when it turns.
pub async fn window(db: &Db, kind: WindowKind) -> anyhow::Result<(DateTime<Utc>, DateTime<Utc>)> {
    let row = sqlx::query(
        "SELECT date_trunc($1, now()) AS start,
                date_trunc($1, now()) + ('1 ' || $1)::interval AS ends",
    )
    .bind(kind.as_str())
    .fetch_one(&db.pool)
    .await?;
    Ok((row.get("start"), row.get("ends")))
}

/// What a policy's scope has spent since `since`, minus any override
/// written in that window.
///
/// Dollars count only what engines reported — a run with no price adds
/// nothing to `usd` and everything to `output_tokens`. Runs are counted once
/// they start; a run still queued has spent nothing.
pub async fn usage(db: &Db, policy: &Policy, since: DateTime<Utc>) -> anyhow::Result<Usage> {
    let used = if policy.scope_kind == ScopeKind::Agent {
        agent_usage(db, policy.scope_id, since).await?
    } else {
        // Each arm is a literal: the scope id is always bound, never written in.
        let filter = match policy.scope_kind {
            ScopeKind::Machine => "TRUE",
            ScopeKind::Workspace => "COALESCE(p.workspace_id, c.workspace_id) = $2",
            ScopeKind::Project => "p.id = $2",
            ScopeKind::Team => "COALESCE(r.team_id, t.team_id) = $2",
            ScopeKind::Routine => {
                "(r.id IN (SELECT run_id FROM routine_runs WHERE routine_id = $2 AND run_id IS NOT NULL)
                  OR r.task_id IN (SELECT task_id FROM routine_runs
                                    WHERE routine_id = $2 AND task_id IS NOT NULL))"
            }
            ScopeKind::Agent => unreachable!("answered by agent_usage"),
        };
        let row = sqlx::query(&format!(
            "SELECT COALESCE(SUM(r.cost_usd), 0) AS usd,
                    COALESCE(SUM(r.output_tokens), 0)::bigint AS tokens,
                    COUNT(*) FILTER (WHERE r.started_at IS NOT NULL) AS runs
               FROM runs r {PROJECT_JOIN}
              WHERE COALESCE(r.finished_at, r.started_at, r.created_at) >= $1 AND {filter}"
        ))
        .bind(since)
        .bind(policy.scope_id)
        .fetch_one(&db.pool)
        .await?;
        Usage {
            usd: row.get("usd"),
            output_tokens: row.get("tokens"),
            runs: row.get("runs"),
        }
    };
    let credit = sqlx::query(
        "SELECT COALESCE(SUM(usd), 0) AS usd, COALESCE(SUM(tokens), 0)::bigint AS tokens,
                COALESCE(SUM(runs), 0)::bigint AS runs
           FROM budget_incidents
          WHERE policy_id = $1 AND kind = 'override' AND window_start = $2",
    )
    .bind(policy.id)
    .bind(since)
    .fetch_one(&db.pool)
    .await?;
    Ok(Usage {
        usd: (used.usd - credit.get::<f64, _>("usd")).max(0.0),
        output_tokens: (used.output_tokens - credit.get::<i64, _>("tokens")).max(0),
        runs: (used.runs - credit.get::<i64, _>("runs")).max(0),
    })
}

/// An agent's spend the way the spend page counts it: runs it did alone,
/// and its own steps of team runs — never a whole team run's cost.
async fn agent_usage(db: &Db, agent: Option<Uuid>, since: DateTime<Utc>) -> anyhow::Result<Usage> {
    let row = sqlx::query(&format!(
        "SELECT COALESCE(SUM(usd), 0) AS usd, COALESCE(SUM(tokens), 0)::bigint AS tokens,
                COUNT(*) FILTER (WHERE started) AS runs
           FROM (
             SELECT r.cost_usd AS usd, r.output_tokens AS tokens, r.started_at IS NOT NULL AS started
               FROM runs r LEFT JOIN tasks t ON t.id = r.task_id
              WHERE r.team_id IS NULL AND r.workflow_id IS NULL
                AND COALESCE(r.agent_id, t.agent_id) = $2
                AND COALESCE(r.finished_at, r.started_at, r.created_at) >= $1
             UNION ALL
             SELECT s.cost_usd, s.output_tokens, s.started_at IS NOT NULL
               FROM steps s
               JOIN agents a ON a.id = $2 AND s.assignee = a.name
               JOIN runs r ON r.id = s.run_id {PROJECT_JOIN}
              WHERE p.workspace_id = a.workspace_id
                AND COALESCE(s.finished_at, s.started_at, r.created_at) >= $1
           ) work"
    ))
    .bind(since)
    .bind(agent)
    .fetch_one(&db.pool)
    .await?;
    Ok(Usage {
        usd: row.get("usd"),
        output_tokens: row.get("tokens"),
        runs: row.get("runs"),
    })
}

/// Where one policy stands right now — for the budget screen and the gate.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Standing {
    pub policy: Policy,
    pub used: Usage,
    pub verdict: Verdict,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
}

pub async fn standing(db: &Db, policy: Policy) -> anyhow::Result<Standing> {
    let (window_start, window_end) = window(db, policy.window_kind).await?;
    let used = usage(db, &policy, window_start).await?;
    Ok(Standing {
        verdict: evaluate(&policy, &used),
        policy,
        used,
        window_start,
        window_end,
    })
}

/// Check a scope's policies and refuse if one is spent.
///
/// Records what it found: the first time in a window that a policy passes
/// its warning line or is spent, an incident is written and the attention
/// hook told — once, however many times this is asked.
pub async fn check(db: &Db, scope: &Scope, with_machine: bool) -> Result<(), OverBudget> {
    let policies = match policies_for(db, scope, with_machine).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "budget read failed; letting the work through");
            return Ok(());
        }
    };
    for policy in policies {
        let standing = match standing(db, policy.clone()).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(policy = %policy.name, error = %e, "budget read failed; letting the work through");
                continue;
            }
        };
        match standing.verdict {
            Verdict::Open => {}
            Verdict::Warn { percent } => {
                if record_once(db, &standing, "warn").await {
                    crate::attention::fire(
                        db,
                        crate::attention::Event::BudgetWarning,
                        crate::attention::Ctx {
                            title: format!(
                                "aichip: budget \u{201c}{}\u{201d} is {percent}% spent",
                                policy.name
                            ),
                            body: format!(
                                "{} this {}",
                                summary(&standing.policy, &standing.used),
                                policy.window_kind.as_str()
                            ),
                            ..Default::default()
                        },
                    )
                    .await;
                }
            }
            Verdict::Exceeded { cap } => {
                let over = over_budget(&standing, cap);
                if record_once(db, &standing, "exceeded").await {
                    crate::attention::fire(
                        db,
                        crate::attention::Event::OverBudget,
                        crate::attention::Ctx {
                            title: format!(
                                "aichip: budget \u{201c}{}\u{201d} is spent",
                                policy.name
                            ),
                            body: over.to_string(),
                            ..Default::default()
                        },
                    )
                    .await;
                }
                return Err(over);
            }
        }
    }
    Ok(())
}

/// Every cap a policy has, for a notification body.
fn summary(policy: &Policy, used: &Usage) -> String {
    let mut parts = vec![];
    if policy.cap_usd.is_some() {
        parts.push(describe(policy, used, Cap::Usd));
    }
    if policy.cap_output_tokens.is_some() {
        parts.push(describe(policy, used, Cap::Tokens));
    }
    if policy.cap_runs.is_some() {
        parts.push(describe(policy, used, Cap::Runs));
    }
    parts.join(", ")
}

fn over_budget(standing: &Standing, cap: Cap) -> OverBudget {
    OverBudget {
        policy: standing.policy.name.clone(),
        policy_id: standing.policy.id,
        window: standing.policy.window_kind.as_str(),
        detail: describe(&standing.policy, &standing.used, cap),
        resets: resets_phrase(standing.policy.window_kind),
        resets_at: standing.window_end,
    }
}

fn resets_phrase(kind: WindowKind) -> String {
    match kind {
        WindowKind::Day => "at midnight",
        WindowKind::Week => "at the start of next week",
        WindowKind::Month => "at the start of next month",
    }
    .to_string()
}

/// Write a `warn` or `exceeded` incident unless this window already has one.
/// True when this call wrote it — the caller's cue to notify.
async fn record_once(db: &Db, standing: &Standing, kind: &str) -> bool {
    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO budget_incidents (policy_id, window_start, kind, usd, tokens, runs)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (policy_id, window_start, kind) WHERE kind IN ('warn', 'exceeded') DO NOTHING
         RETURNING id",
    )
    .bind(standing.policy.id)
    .bind(standing.window_start)
    .bind(kind)
    .bind(standing.used.usd)
    .bind(standing.used.output_tokens)
    .bind(standing.used.runs as i32)
    .fetch_optional(&db.pool)
    .await
    .map(|written| written.is_some())
    .unwrap_or(false)
}

/// How far a run may go before a `stop` policy halts it: the smallest token
/// headroom among the policies covering it, with the policy's name.
pub async fn token_headroom(db: &Db, run_id: Uuid) -> Option<(i64, String)> {
    let scope = scope_of_run(db, run_id).await.ok()?;
    let policies = policies_for(db, &scope, true).await.ok()?;
    let mut tightest: Option<(i64, String)> = None;
    for policy in policies.into_iter().filter(|p| p.stops_in_flight) {
        let Some(cap) = policy.cap_output_tokens else {
            continue;
        };
        let Ok(standing) = standing(db, policy).await else {
            continue;
        };
        let left = (cap - standing.used.output_tokens).max(0);
        if tightest.as_ref().is_none_or(|(t, _)| left < *t) {
            tightest = Some((left, standing.policy.name));
        }
    }
    tightest
}

/// The first machine-wide policy that is spent, if any — what stops the
/// whole queue. A read only: `claim_next`'s `check` is what records it.
pub async fn machine_gate(db: &Db) -> Option<OverBudget> {
    let policies = policies_for(db, &Scope::default(), true).await.ok()?;
    for policy in policies {
        let Ok(standing) = standing(db, policy).await else {
            continue;
        };
        if let Verdict::Exceeded { cap } = standing.verdict {
            return Some(over_budget(&standing, cap));
        }
    }
    None
}

/// The policy the old daily cap became.
const DAILY: &str = "Daily budget";

/// The machine's daily dollar cap, for what still asks for one number.
pub async fn daily_cap(db: &Db) -> Option<f64> {
    sqlx::query_scalar::<_, Option<f64>>(
        "SELECT cap_usd FROM budget_policies
          WHERE enabled AND scope_kind = 'machine' AND window_kind = 'day' AND name = $1
          ORDER BY created_at LIMIT 1",
    )
    .bind(DAILY)
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten()
    .flatten()
}

/// Set or clear the machine's daily dollar cap — the old control, kept
/// working as an edit of the "Daily budget" policy.
pub async fn set_daily_cap(db: &Db, cap: Option<f64>) -> anyhow::Result<()> {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM budget_policies
          WHERE scope_kind = 'machine' AND window_kind = 'day' AND name = $1
          ORDER BY created_at LIMIT 1",
    )
    .bind(DAILY)
    .fetch_optional(&db.pool)
    .await?;
    match (existing, cap) {
        (Some(id), Some(cap)) => {
            sqlx::query("UPDATE budget_policies SET cap_usd = $2, enabled = TRUE WHERE id = $1")
                .bind(id)
                .bind(cap)
                .execute(&db.pool)
                .await?;
            release_held(db, id).await?;
        }
        (Some(id), None) => {
            release_held(db, id).await?;
            sqlx::query("DELETE FROM budget_policies WHERE id = $1")
                .bind(id)
                .execute(&db.pool)
                .await?;
        }
        (None, Some(cap)) => {
            sqlx::query(
                "INSERT INTO budget_policies (name, scope_kind, window_kind, cap_usd)
                 VALUES ($1, 'machine', 'day', $2)",
            )
            .bind(DAILY)
            .bind(cap)
            .execute(&db.pool)
            .await?;
        }
        (None, None) => {}
    }
    Ok(())
}

/// Let go of every queued run a policy was holding — after an override, an
/// edit or a delete, the question has a new answer and the queue should ask
/// it again rather than wait out the window.
pub async fn release_held(db: &Db, policy_id: Uuid) -> anyhow::Result<u64> {
    Ok(sqlx::query(
        "UPDATE queue SET not_before = NULL, hold_reason = NULL, held_by = NULL WHERE held_by = $1",
    )
    .bind(policy_id)
    .execute(&db.pool)
    .await?
    .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(usd: Option<f64>, tokens: Option<i64>, runs: Option<i32>) -> Policy {
        Policy {
            id: Uuid::nil(),
            name: "Nightly".into(),
            scope_kind: ScopeKind::Project,
            scope_id: Some(Uuid::nil()),
            window_kind: WindowKind::Week,
            cap_usd: usd,
            cap_output_tokens: tokens,
            cap_runs: runs,
            warn_percent: 80,
            stops_in_flight: false,
            confirm_above_usd: None,
            enabled: true,
        }
    }

    fn used(usd: f64, output_tokens: i64, runs: i64) -> Usage {
        Usage {
            usd,
            output_tokens,
            runs,
        }
    }

    #[test]
    fn under_every_cap_is_open() {
        assert_eq!(
            evaluate(
                &policy(Some(40.0), Some(100_000), Some(10)),
                &used(10.0, 1000, 2)
            ),
            Verdict::Open
        );
    }

    #[test]
    fn at_the_cap_is_spent_because_one_more_would_go_over() {
        assert_eq!(
            evaluate(&policy(Some(40.0), None, None), &used(40.0, 0, 0)),
            Verdict::Exceeded { cap: Cap::Usd }
        );
        assert_eq!(
            evaluate(&policy(None, None, Some(3)), &used(0.0, 0, 3)),
            Verdict::Exceeded { cap: Cap::Runs }
        );
    }

    #[test]
    fn any_one_cap_spent_is_enough() {
        assert_eq!(
            evaluate(&policy(Some(40.0), Some(1000), None), &used(1.0, 5000, 0)),
            Verdict::Exceeded { cap: Cap::Tokens }
        );
    }

    #[test]
    fn the_warning_follows_the_most_spent_cap() {
        assert_eq!(
            evaluate(&policy(Some(100.0), Some(1000), None), &used(10.0, 850, 0)),
            Verdict::Warn { percent: 85 }
        );
        assert_eq!(
            evaluate(&policy(Some(100.0), None, None), &used(79.0, 0, 0)),
            Verdict::Open
        );
    }

    /// The case `reports_cost == false` exists for: an engine that never
    /// prices a run adds nothing to a dollar cap, however much it ran.
    #[test]
    fn unpriced_work_is_invisible_to_a_dollar_cap_and_seen_by_a_token_cap() {
        let unpriced = used(0.0, 2_000_000, 40);
        assert_eq!(
            evaluate(&policy(Some(5.0), None, None), &unpriced),
            Verdict::Open
        );
        assert_eq!(
            evaluate(&policy(Some(5.0), Some(1_000_000), None), &unpriced),
            Verdict::Exceeded { cap: Cap::Tokens }
        );
    }

    #[test]
    fn a_refusal_names_the_policy_the_cap_and_when_it_resets() {
        let p = policy(Some(40.0), None, None);
        let over = OverBudget {
            policy: p.name.clone(),
            policy_id: p.id,
            window: "week",
            detail: describe(&p, &used(41.2, 0, 0), Cap::Usd),
            resets: resets_phrase(WindowKind::Week),
            resets_at: Utc::now(),
        };
        assert_eq!(
            over.to_string(),
            "budget \u{201c}Nightly\u{201d} is spent for this week ($41.20 of $40.00) — it resets at the start of next week; raise it or override it in Activity"
        );
        assert_eq!(
            describe(
                &policy(None, Some(150_000), None),
                &used(0.0, 182_400, 0),
                Cap::Tokens
            ),
            "182k of 150k output tokens"
        );
    }
}

/// Against a real database and the mock engine — see `crate::testdb`.
#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    async fn policy(
        t: &testdb::TestDb,
        kind: &str,
        scope: Option<Uuid>,
        caps: &str,
        on_exceed: &str,
    ) -> Uuid {
        // `caps` is a literal from the tests below, never input.
        sqlx::query_scalar(&format!(
            "INSERT INTO budget_policies (name, scope_kind, scope_id, window_kind, on_exceed, {caps})
             VALUES ('test', $1, $2, 'day', $3, 1) RETURNING id"
        ))
        .bind(kind)
        .bind(scope)
        .bind(on_exceed)
        .fetch_one(&t.db.pool)
        .await
        .unwrap()
    }

    /// One spent project holds its own queued work and only its own: the run
    /// behind it in another project still starts.
    #[tokio::test]
    async fn a_spent_project_holds_its_runs_and_not_the_others() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        let (_, spent) = t.project(dir.path(), true).await;
        let (_, other) = t.project(&dir.path().join("other"), true).await;
        let a = t.card(spent, "spent").await;
        let b = t.card(other, "other").await;

        // Queue both while the queue holds, then spend the first project's
        // one run allowance before letting the queue go.
        orchestrator.set_queue_paused(true).await.unwrap();
        let held = orchestrator.start_card(a).await.unwrap();
        let free = orchestrator.start_card(b).await.unwrap();
        let cap = policy(&t, "project", Some(spent), "cap_runs", "hold").await;
        sqlx::query(
            "INSERT INTO runs (task_id, status, trigger, engine, started_at, finished_at)
             VALUES ($1, 'completed', 'manual', 'mock', now(), now())",
        )
        .bind(a)
        .execute(&t.db.pool)
        .await
        .unwrap();
        orchestrator.set_queue_paused(false).await.unwrap();

        t.until(
            "the other project's run to finish",
            "SELECT status = 'completed' FROM runs WHERE id = $1",
            free,
        )
        .await;
        let (status, reason, held_by): (String, Option<String>, Option<Uuid>) = sqlx::query_as(
            "SELECT r.status, q.hold_reason, q.held_by FROM runs r JOIN queue q ON q.run_id = r.id
              WHERE r.id = $1",
        )
        .bind(held)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        assert_eq!(status, "queued");
        assert_eq!(held_by, Some(cap));
        assert!(reason.unwrap().contains("1 of 1 runs"));

        // An override gives the window one more run, and lets it go now.
        sqlx::query(
            "INSERT INTO budget_incidents (policy_id, window_start, kind, runs)
             VALUES ($1, date_trunc('day', now()), 'override', 1)",
        )
        .bind(cap)
        .execute(&t.db.pool)
        .await
        .unwrap();
        release_held(&t.db, cap).await.unwrap();
        t.until(
            "the held run to start after the override",
            "SELECT status <> 'queued' FROM runs WHERE id = $1",
            held,
        )
        .await;

        t.finish().await;
    }

    /// At the click, a spent budget refuses with its name rather than
    /// queueing the card to wait out the window — and says it once.
    #[tokio::test]
    async fn a_spent_budget_refuses_at_the_click_and_notes_it_once() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        let (_, project) = t.project(dir.path(), true).await;
        let card = t.card(project, "work").await;
        let cap = policy(&t, "machine", None, "cap_runs", "hold").await;
        sqlx::query(
            "INSERT INTO runs (task_id, status, trigger, engine, started_at)
             VALUES ($1, 'completed', 'manual', 'mock', now())",
        )
        .bind(card)
        .execute(&t.db.pool)
        .await
        .unwrap();

        for _ in 0..3 {
            let refused = orchestrator.start_card(card).await.unwrap_err();
            let over = refused
                .downcast_ref::<OverBudget>()
                .expect("refused as over budget");
            assert_eq!(over.policy_id, cap);
        }
        let incidents: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM budget_incidents WHERE policy_id = $1 AND kind = 'exceeded'",
        )
        .bind(cap)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        assert_eq!(incidents, 1, "once per window, however often it is asked");
        assert!(matches!(
            orchestrator.queue_gate().await,
            crate::runs::orchestrator::QueueGate::OverBudget(_)
        ));

        t.finish().await;
    }

    /// A `stop` policy's token cap stops a run mid-stream; dollars could not.
    #[tokio::test]
    async fn a_stop_policy_stops_a_run_that_crosses_its_token_cap() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        let (_, project) = t.project(dir.path(), true).await;
        let card = t.card(project, "chatty").await;
        // The demo replay reaches 97 output tokens before it finishes.
        let cap = policy(&t, "machine", None, "cap_output_tokens", "stop").await;
        sqlx::query("UPDATE budget_policies SET cap_output_tokens = 50 WHERE id = $1")
            .bind(cap)
            .execute(&t.db.pool)
            .await
            .unwrap();
        let run = orchestrator.start_card(card).await.unwrap();
        t.until(
            "the run to end",
            "SELECT status IN ('completed','failed','canceled') FROM runs WHERE id = $1",
            run,
        )
        .await;
        let (status, reason): (String, Option<String>) =
            sqlx::query_as("SELECT status, error_reason FROM runs WHERE id = $1")
                .bind(run)
                .fetch_one(&t.db.pool)
                .await
                .unwrap();
        assert_eq!(status, "failed");
        assert!(reason.unwrap_or_default().contains("stopped by budget"));

        t.finish().await;
    }

    /// The old control keeps working, as an edit of the "Daily budget" policy.
    #[tokio::test]
    async fn the_daily_cap_is_a_policy_now() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        assert_eq!(daily_cap(&t.db).await, None);
        set_daily_cap(&t.db, Some(12.5)).await.unwrap();
        assert_eq!(daily_cap(&t.db).await, Some(12.5));
        let p = list(&t.db).await.unwrap();
        assert_eq!(
            (p.len(), p[0].scope_kind, p[0].window_kind),
            (1, ScopeKind::Machine, WindowKind::Day)
        );
        set_daily_cap(&t.db, None).await.unwrap();
        assert!(list(&t.db).await.unwrap().is_empty());
        t.finish().await;
    }
}
