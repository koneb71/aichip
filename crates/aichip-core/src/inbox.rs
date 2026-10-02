//! Everything waiting on a person, in one list.
//!
//! A query over the rows that already say they are waiting — never an index
//! of its own. Plans, chat questions, schema plans and the rest each have a
//! status column that is the truth about them; a table written beside each
//! transition would be a second truth, free to disagree with the first the
//! first time a transition forgot to write it. (`apps::schema::plan` diffs
//! against `information_schema` rather than a registry for the same reason.)
//!
//! Only what had no row of its own got one — permission prompts, which lived
//! in memory, and an agent's questions and proposals, which did not exist.
//!
//! Resolving lives in the server, because half the answers are route handlers
//! and the broker; listing is all here.

use crate::db::Db;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

/// What an item is, which decides what can be done with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A card's plan, parked for approval.
    Plan,
    /// A team run's plan, parked for approval.
    TeamPlan,
    /// A run waiting, right now, for a tool to be allowed.
    Permission,
    /// A permission prompt the server restarted under. The run failed; it can
    /// be resumed.
    PermissionExpired,
    /// A card's agent asked something.
    Question,
    /// An agent proposed something only a person can do.
    Decision,
    /// The chat assistant asked something.
    ChatQuestion,
    /// The chat assistant proposed a plan.
    ChatPlan,
    /// An app's schema change that would lose data.
    Schema,
    /// An agent's edit to a knowledge-base page.
    KbRevision,
    /// A preview recipe an agent wrote.
    Recipe,
}

impl Kind {
    /// What can be done from the inbox itself; anything else is "open".
    pub fn actions(self) -> &'static [&'static str] {
        match self {
            Kind::Plan => &["approve", "revise"],
            Kind::TeamPlan => &["approve", "reject"],
            Kind::Permission => &["allow", "deny"],
            Kind::PermissionExpired => &["resume", "dismiss"],
            Kind::Question => &["answer", "dismiss"],
            Kind::Decision => &["approve", "deny"],
            Kind::ChatQuestion => &[],
            Kind::ChatPlan => &["approve"],
            Kind::Schema => &["apply", "discard"],
            Kind::KbRevision => &["accept", "discard"],
            Kind::Recipe => &[],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Plan => "plan",
            Kind::TeamPlan => "team_plan",
            Kind::Permission => "permission",
            Kind::PermissionExpired => "permission_expired",
            Kind::Question => "question",
            Kind::Decision => "decision",
            Kind::ChatQuestion => "chat_question",
            Kind::ChatPlan => "chat_plan",
            Kind::Schema => "schema",
            Kind::KbRevision => "kb_revision",
            Kind::Recipe => "recipe",
        }
    }
}

/// One thing waiting on a person.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// `kind:id[:id]`, stable for as long as the thing waits. What resolve,
    /// read and snooze address.
    pub key: String,
    pub kind: Kind,
    pub title: String,
    pub detail: Option<String>,
    pub project_id: Option<Uuid>,
    pub project_name: Option<String>,
    pub task_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    /// Where "open" goes, as a dashboard path.
    pub link: String,
    pub created_at: DateTime<Utc>,
    pub actions: &'static [&'static str],
    pub read: bool,
    pub snoozed_until: Option<DateTime<Utc>>,
    /// Options a question offered, for one-click answers.
    pub options: Vec<String>,
}

/// A parsed key. Pure, for the tests and for the resolve route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub kind: String,
    pub ids: Vec<String>,
}

pub fn parse_key(key: &str) -> Option<Key> {
    let mut parts = key.split(':');
    let kind = parts.next().filter(|k| !k.is_empty())?.to_string();
    let ids: Vec<String> = parts.map(str::to_string).collect();
    if ids.is_empty() || ids.iter().any(|i| i.is_empty()) {
        return None;
    }
    Some(Key { kind, ids })
}

impl Key {
    pub fn uuid(&self, i: usize) -> Option<Uuid> {
        self.ids.get(i).and_then(|s| Uuid::parse_str(s).ok())
    }
}

fn card_link(project: Option<Uuid>, task: Option<Uuid>) -> String {
    match (project, task) {
        (Some(p), Some(t)) => format!("/projects/{p}?task={t}"),
        (Some(p), None) => format!("/projects/{p}"),
        _ => "/activity".into(),
    }
}

fn clip(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > max {
        format!("{}…", one.chars().take(max).collect::<String>())
    } else {
        one
    }
}

/// The project of a run, however it reaches one; and its workspace.
const RUN_PROJECT: &str = "
    LEFT JOIN tasks t ON t.id = r.task_id
    LEFT JOIN workflows w ON w.id = r.workflow_id
    LEFT JOIN projects p ON p.id = COALESCE(r.project_id, t.project_id, w.project_id)";

#[allow(clippy::too_many_arguments)]
fn make(
    kind: Kind,
    key: String,
    title: String,
    detail: Option<String>,
    project: (Option<Uuid>, Option<String>),
    task: Option<Uuid>,
    run: Option<Uuid>,
    link: String,
    at: DateTime<Utc>,
    options: Vec<String>,
) -> Item {
    Item {
        key,
        kind,
        title,
        detail,
        project_id: project.0,
        project_name: project.1,
        task_id: task,
        run_id: run,
        link,
        created_at: at,
        actions: kind.actions(),
        read: false,
        snoozed_until: None,
        options,
    }
}

/// Everything waiting in a workspace, newest first. Snoozed items are left
/// out unless asked for.
pub async fn list(db: &Db, workspace: Uuid, include_snoozed: bool) -> anyhow::Result<Vec<Item>> {
    let mut items: Vec<Item> = Vec::new();

    // Plans, card and team alike: a parked run.
    for r in sqlx::query(&format!(
        "SELECT r.id, r.team_id, r.task_id, r.goal, t.title, p.id AS pid, p.name AS pname,
                COALESCE(s.finished_at, r.created_at) AS at
           FROM runs r {RUN_PROJECT}
           LEFT JOIN steps s ON s.run_id = r.id AND s.step_key = 'plan'
          WHERE r.status = 'awaiting_approval' AND p.workspace_id = $1"
    ))
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let run: Uuid = r.get("id");
        let team = r.get::<Option<Uuid>, _>("team_id").is_some();
        let title = r
            .get::<Option<String>, _>("title")
            .or_else(|| r.get::<Option<String>, _>("goal").map(|g| clip(&g, 80)))
            .unwrap_or_else(|| "A run".into());
        items.push(make(
            if team { Kind::TeamPlan } else { Kind::Plan },
            format!("{}:{run}", if team { "team_plan" } else { "plan" }),
            title,
            Some(if team {
                "A team's plan is ready for review".into()
            } else {
                "A plan is ready for review".into()
            }),
            (r.get("pid"), r.get("pname")),
            r.get("task_id"),
            Some(run),
            card_link(r.get("pid"), r.get("task_id")),
            r.get("at"),
            vec![],
        ));
    }

    // Permission prompts: live ones, then the ones a restart cut off that
    // nothing has resumed since.
    for r in sqlx::query(&format!(
        "SELECT pr.id AS rid, pr.tool, pr.input_summary, pr.created_at, pr.decision, r.id, r.task_id,
                t.title, p.id AS pid, p.name AS pname
           FROM permission_requests pr JOIN runs r ON r.id = pr.run_id {RUN_PROJECT}
          WHERE p.workspace_id = $1
            AND ((pr.resolved_at IS NULL AND r.status = 'waiting_permission')
              OR (pr.decision = 'expired' AND pr.created_at > now() - interval '7 days'
                  AND NOT EXISTS (SELECT 1 FROM runs x WHERE x.resumed_from = r.id)))"
    ))
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let expired = r.get::<Option<String>, _>("decision").as_deref() == Some("expired");
        let run: Uuid = r.get("id");
        let tool: String = r.get("tool");
        let card = r.get::<Option<String>, _>("title");
        items.push(make(
            if expired { Kind::PermissionExpired } else { Kind::Permission },
            if expired { format!("permission_expired:{run}") } else { format!("permission:{}", r.get::<String, _>("rid")) },
            if expired {
                format!("Never answered: allow {tool}?")
            } else {
                format!("Allow {tool}?")
            },
            Some(match (expired, card) {
                (true, Some(c)) => format!("{c} — the server restarted while this waited. Resume picks the run up where it was."),
                (true, None) => "The server restarted while this waited. Resume picks the run up where it was.".into(),
                (false, _) => r.get::<String, _>("input_summary"),
            }),
            (r.get("pid"), r.get("pname")),
            r.get("task_id"),
            Some(run),
            card_link(r.get("pid"), r.get("task_id")),
            r.get("created_at"),
            vec![],
        ));
    }
    // One expired item per run: a run that parked on three prompts at once
    // is resumed once.
    let mut seen = std::collections::HashSet::new();
    items.retain(|i| i.kind != Kind::PermissionExpired || seen.insert(i.key.clone()));

    // A card agent's questions.
    for r in sqlx::query(
        "SELECT q.id, q.question, q.options, q.run_id, q.created_at, t.id AS tid, t.title,
                p.id AS pid, p.name AS pname
           FROM run_questions q JOIN tasks t ON t.id = q.task_id JOIN projects p ON p.id = t.project_id
          WHERE q.answered_at IS NULL AND p.workspace_id = $1",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let options: Vec<String> = serde_json::from_value(r.get("options")).unwrap_or_default();
        items.push(make(
            Kind::Question,
            format!("question:{}", r.get::<Uuid, _>("id")),
            r.get("question"),
            Some(format!("Asked on “{}”", r.get::<String, _>("title"))),
            (r.get("pid"), r.get("pname")),
            Some(r.get("tid")),
            Some(r.get("run_id")),
            card_link(r.get("pid"), Some(r.get("tid"))),
            r.get("created_at"),
            options,
        ));
    }

    // Proposals.
    for r in sqlx::query(
        "SELECT d.id, d.effect, d.reason, d.run_id, d.proposed_by, d.created_at, d.project_id,
                p.name AS pname
           FROM decisions d LEFT JOIN projects p ON p.id = d.project_id
          WHERE d.status = 'open' AND d.workspace_id = $1",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let effect: Option<crate::decisions::Effect> = serde_json::from_value(r.get("effect")).ok();
        let title = match &effect {
            Some(e) => crate::decisions::describe(db, e).await,
            None => "A proposal this version cannot read".into(),
        };
        let card = effect.as_ref().and_then(|e| e.cards().first().copied());
        let pid: Option<Uuid> = r.get("project_id");
        items.push(make(
            Kind::Decision,
            format!("decision:{}", r.get::<Uuid, _>("id")),
            title,
            Some(format!(
                "{} — {}",
                r.get::<String, _>("proposed_by"),
                r.get::<String, _>("reason")
            )),
            (pid, r.get("pname")),
            card,
            r.get("run_id"),
            card_link(pid, card),
            r.get("created_at"),
            vec![],
        ));
    }

    // The chat assistant's open questions and plans.
    for r in sqlx::query(
        "SELECT cq.id, cq.chat_id, cq.questions, cq.created_at, c.title, c.project_id, p.name AS pname
           FROM chat_questions cq JOIN chats c ON c.id = cq.chat_id
           LEFT JOIN projects p ON p.id = c.project_id
          WHERE cq.answered_at IS NULL AND COALESCE(p.workspace_id, c.workspace_id) = $1",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let questions: serde_json::Value = r.get("questions");
        let first = questions[0]["question"].as_str().unwrap_or("The assistant asked something").to_string();
        let pid: Option<Uuid> = r.get("project_id");
        items.push(make(
            Kind::ChatQuestion,
            format!("chat_question:{}:{}", r.get::<Uuid, _>("chat_id"), r.get::<Uuid, _>("id")),
            first,
            Some(format!("In the chat “{}” — answer it there", r.get::<String, _>("title"))),
            (pid, r.get("pname")),
            None,
            None,
            pid.map(|p| format!("/projects/{p}")).unwrap_or_else(|| "/chat".into()),
            r.get("created_at"),
            vec![],
        ));
    }
    for r in sqlx::query(
        "SELECT m.id, m.chat_id, m.content, m.created_at, c.title, c.project_id, p.name AS pname
           FROM chat_messages m JOIN chats c ON c.id = m.chat_id
           LEFT JOIN projects p ON p.id = c.project_id
          WHERE m.is_plan AND m.plan_outcome IS NULL
            AND m.created_at > now() - interval '30 days'
            AND COALESCE(p.workspace_id, c.workspace_id) = $1",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let pid: Option<Uuid> = r.get("project_id");
        items.push(make(
            Kind::ChatPlan,
            format!(
                "chat_plan:{}:{}",
                r.get::<Uuid, _>("chat_id"),
                r.get::<Uuid, _>("id")
            ),
            format!("A plan in “{}”", r.get::<String, _>("title")),
            Some(clip(&r.get::<String, _>("content"), 200)),
            (pid, r.get("pname")),
            None,
            None,
            pid.map(|p| format!("/projects/{p}"))
                .unwrap_or_else(|| "/chat".into()),
            r.get("created_at"),
            vec![],
        ));
    }

    // An app's schema change that waits because it would lose data.
    for r in sqlx::query(
        "SELECT s.id, s.app_id, s.created_at, a.name, a.project_id, p.name AS pname
           FROM app_schema_plans s JOIN apps a ON a.id = s.app_id
           LEFT JOIN projects p ON p.id = a.project_id
          WHERE s.status = 'pending' AND a.workspace_id = $1",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let app: Uuid = r.get("app_id");
        items.push(make(
            Kind::Schema,
            format!("schema:{app}:{}", r.get::<Uuid, _>("id")),
            format!("Schema change for {}", r.get::<String, _>("name")),
            Some("It would drop or change data, so it waits for you".into()),
            (r.get("project_id"), r.get("pname")),
            None,
            None,
            format!("/apps/{app}"),
            r.get("created_at"),
            vec![],
        ));
    }

    // An agent's edit to a knowledge-base page.
    for r in sqlx::query(
        "SELECT kr.article_id, kr.seq, kr.note, kr.created_at, k.title
           FROM kb_revisions kr JOIN kb_articles k ON k.id = kr.article_id
          WHERE kr.state = 'pending' AND k.workspace_id = $1",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let article: Uuid = r.get("article_id");
        let note: String = r.get("note");
        items.push(make(
            Kind::KbRevision,
            format!("kb_revision:{article}:{}", r.get::<i32, _>("seq")),
            format!("Suggested edit to “{}”", r.get::<String, _>("title")),
            (!note.trim().is_empty()).then(|| clip(&note, 200)),
            (None, None),
            None,
            None,
            format!("/knowledge/{article}"),
            r.get("created_at"),
            vec![],
        ));
    }

    // A preview recipe an agent wrote: it builds a container, so a person
    // reads it first — on the project, where the recipe is shown in full.
    for r in sqlx::query(
        "SELECT pr.project_id, pr.kind, pr.created_at, p.name
           FROM preview_recipes pr JOIN projects p ON p.id = pr.project_id
          WHERE pr.status = 'proposed' AND p.workspace_id = $1",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?
    {
        let pid: Uuid = r.get("project_id");
        items.push(make(
            Kind::Recipe,
            format!("recipe:{pid}:{}", r.get::<String, _>("kind")),
            format!("Preview recipe for {}", r.get::<String, _>("name")),
            Some("Read it before it builds anything".into()),
            (Some(pid), Some(r.get("name"))),
            None,
            None,
            format!("/projects/{pid}"),
            r.get("created_at"),
            vec![],
        ));
    }

    // Marks, applied last.
    let keys: Vec<String> = items.iter().map(|i| i.key.clone()).collect();
    let marks: HashMap<String, Marks> =
        sqlx::query("SELECT key, read_at, snoozed_until FROM inbox_marks WHERE key = ANY($1)")
            .bind(&keys)
            .fetch_all(&db.pool)
            .await?
            .into_iter()
            .map(|r| (r.get("key"), (r.get("read_at"), r.get("snoozed_until"))))
            .collect();
    let now = Utc::now();
    for item in &mut items {
        if let Some((read, snoozed)) = marks.get(&item.key) {
            item.read = read.is_some();
            item.snoozed_until = snoozed.filter(|s| *s > now);
        }
    }
    if !include_snoozed {
        items.retain(|i| i.snoozed_until.is_none());
    }
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(items)
}

/// When an item was read, and until when it is snoozed.
type Marks = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

pub async fn mark_read(db: &Db, key: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO inbox_marks (key, read_at) VALUES ($1, now())
         ON CONFLICT (key) DO UPDATE SET read_at = now()",
    )
    .bind(key)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Out of the list until `until`. A far-future time is how "dismiss" works
/// for things that have no state of their own to close.
pub async fn snooze(db: &Db, key: &str, until: DateTime<Utc>) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO inbox_marks (key, snoozed_until) VALUES ($1, $2)
         ON CONFLICT (key) DO UPDATE SET snoozed_until = $2",
    )
    .bind(key)
    .bind(until)
    .execute(&db.pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_refuse_nonsense() {
        let k = parse_key("schema:a:b").unwrap();
        assert_eq!(k.kind, "schema");
        assert_eq!(k.ids, vec!["a", "b"]);
        assert!(parse_key("plan").is_none());
        assert!(parse_key(":x").is_none());
        assert!(parse_key("plan::x").is_none());
        let id = Uuid::new_v4();
        assert_eq!(parse_key(&format!("plan:{id}")).unwrap().uuid(0), Some(id));
    }

    #[test]
    fn only_a_question_is_answered_in_the_inbox_and_merging_is_never_an_action() {
        for kind in [
            Kind::Plan,
            Kind::TeamPlan,
            Kind::Permission,
            Kind::PermissionExpired,
            Kind::Question,
            Kind::Decision,
            Kind::ChatQuestion,
            Kind::ChatPlan,
            Kind::Schema,
            Kind::KbRevision,
            Kind::Recipe,
        ] {
            assert!(!kind.actions().contains(&"merge"), "{}", kind.as_str());
            assert_eq!(
                kind.actions().contains(&"answer"),
                kind == Kind::Question,
                "{}",
                kind.as_str()
            );
        }
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    async fn run(t: &testdb::TestDb, task: Uuid, status: &str) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO runs (task_id, status, trigger, engine) VALUES ($1, $2, 'manual', 'mock') RETURNING id",
        )
        .bind(task)
        .bind(status)
        .fetch_one(&t.db.pool)
        .await
        .unwrap()
    }

    fn kinds(items: &[Item]) -> Vec<&'static str> {
        let mut k: Vec<_> = items.iter().map(|i| i.kind.as_str()).collect();
        k.sort_unstable();
        k
    }

    /// Each kind of waiting thing shows up once, in its own workspace only.
    #[tokio::test]
    async fn everything_waiting_is_listed_once_and_only_in_its_workspace() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, project) = t.project(dir.path(), true).await;
        let (other_ws, _) = t.project(&dir.path().join("o"), true).await;
        let card = t.card(project, "the card").await;

        run(&t, card, "awaiting_approval").await;
        let team: Uuid = sqlx::query_scalar("INSERT INTO teams (name, pattern, definition, workspace_id) VALUES ('T', 'org', '{}', $1) RETURNING id")
            .bind(ws)
            .fetch_one(&t.db.pool)
            .await
            .unwrap();
        let team_run = run(&t, card, "awaiting_approval").await;
        sqlx::query("UPDATE runs SET team_id = $2 WHERE id = $1")
            .bind(team_run)
            .bind(team)
            .execute(&t.db.pool)
            .await
            .unwrap();
        let waiting = run(&t, card, "waiting_permission").await;
        sqlx::query(
            "INSERT INTO permission_requests (id, run_id, tool) VALUES ('req-1', $1, 'Bash')",
        )
        .bind(waiting)
        .execute(&t.db.pool)
        .await
        .unwrap();
        let asker = run(&t, card, "completed").await;
        crate::asks::ask(
            &t.db,
            asker,
            card,
            "keep the old API?",
            &["yes".into(), "no".into()],
        )
        .await
        .unwrap();
        crate::decisions::propose(
            &t.db,
            ws,
            Some(project),
            Some(asker),
            "Ada",
            &crate::decisions::Effect::MoveCard {
                card_id: card,
                column: "review".into(),
            },
            "it is done",
        )
        .await
        .unwrap();
        let chat: Uuid = sqlx::query_scalar(
            "INSERT INTO chats (project_id, title) VALUES ($1, 'Talk') RETURNING id",
        )
        .bind(project)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO chat_questions (chat_id, questions) VALUES ($1, '[{\"question\":\"which?\",\"options\":[]}]')")
            .bind(chat)
            .execute(&t.db.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_messages (chat_id, role, content, is_plan) VALUES ($1, 'assistant', 'the plan', TRUE)")
            .bind(chat)
            .execute(&t.db.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO preview_recipes (project_id, dockerfile, status, edited, kind) VALUES ($1, 'FROM x', 'proposed', FALSE, 'base')")
            .bind(project)
            .execute(&t.db.pool)
            .await
            .unwrap();

        let items = list(&t.db, ws, false).await.unwrap();
        assert_eq!(
            kinds(&items),
            [
                "chat_plan",
                "chat_question",
                "decision",
                "permission",
                "plan",
                "question",
                "recipe",
                "team_plan"
            ]
        );
        let question = items.iter().find(|i| i.kind == Kind::Question).unwrap();
        assert_eq!(question.options, ["yes", "no"]);
        assert_eq!(question.link, format!("/projects/{project}?task={card}"));
        assert!(
            list(&t.db, other_ws, false).await.unwrap().is_empty(),
            "nothing leaks across workspaces"
        );

        // Snoozed is out of the list, and back with `all`.
        let key = question.key.clone();
        snooze(&t.db, &key, Utc::now() + chrono::Duration::hours(1))
            .await
            .unwrap();
        assert!(!list(&t.db, ws, false)
            .await
            .unwrap()
            .iter()
            .any(|i| i.key == key));
        assert!(list(&t.db, ws, true)
            .await
            .unwrap()
            .iter()
            .any(|i| i.key == key));
        t.finish().await;
    }

    /// A restart cannot keep a prompt alive, but it can keep the question:
    /// the run fails as it always did, and the inbox offers to resume it.
    #[tokio::test]
    async fn a_prompt_the_server_restarted_under_becomes_resumable() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orch = t.orchestrator(dir.path());
        let (ws, project) = t.project(dir.path(), true).await;
        let card = t.card(project, "asked").await;
        let waiting = run(&t, card, "waiting_permission").await;
        for id in ["a", "b"] {
            sqlx::query(
                "INSERT INTO permission_requests (id, run_id, tool) VALUES ($1, $2, 'Bash')",
            )
            .bind(id)
            .bind(waiting)
            .execute(&t.db.pool)
            .await
            .unwrap();
        }
        orch.recover_orphans().await.unwrap();
        let items = list(&t.db, ws, false).await.unwrap();
        assert_eq!(
            kinds(&items),
            ["permission_expired"],
            "one per run, however many prompts it held"
        );
        assert_eq!(items[0].key, format!("permission_expired:{waiting}"));
        let open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM permission_requests WHERE resolved_at IS NULL",
        )
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        assert_eq!(open, 0);

        // Once something resumes it, it is no longer waiting.
        sqlx::query("INSERT INTO runs (task_id, status, trigger, engine, resumed_from) VALUES ($1, 'queued', 'resume', 'mock', $2)")
            .bind(card)
            .bind(waiting)
            .execute(&t.db.pool)
            .await
            .unwrap();
        assert!(list(&t.db, ws, false).await.unwrap().is_empty());
        t.finish().await;
    }

    /// An answer goes back to work in the card's worktree, fenced; one that
    /// cannot start a run is not kept, so the question stays in the inbox.
    #[tokio::test]
    async fn an_answer_returns_to_the_same_worktree_or_is_not_taken() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orch = t.orchestrator(dir.path());
        orch.set_queue_paused(true).await.unwrap();
        let (ws, project) = t.project(dir.path(), true).await;
        let card = t.card(project, "asked").await;
        sqlx::query("UPDATE tasks SET board_column = 'review' WHERE id = $1")
            .bind(card)
            .execute(&t.db.pool)
            .await
            .unwrap();
        let asker = run(&t, card, "completed").await;
        let q = crate::asks::ask(&t.db, asker, card, "keep it?", &[])
            .await
            .unwrap();

        // No worktree: refused, and the question is still open.
        let refused = crate::asks::answer(&orch, q, "yes").await.unwrap_err();
        assert!(
            matches!(refused, crate::approvals::Refusal::Gated(_)),
            "{refused:?}"
        );
        assert_eq!(kinds(&list(&t.db, ws, false).await.unwrap()), ["question"]);

        // With one, the answer runs there, carrying the person's words fenced.
        let tree = tempfile::tempdir().unwrap();
        sqlx::query("UPDATE tasks SET worktree_path = $2 WHERE id = $1")
            .bind(card)
            .bind(tree.path().to_string_lossy().as_ref())
            .execute(&t.db.pool)
            .await
            .unwrap();
        let follow = crate::asks::answer(&orch, q, "yes — keep it")
            .await
            .unwrap();
        let (trigger, prompt): (String, String) =
            sqlx::query_as("SELECT trigger, prompt_override FROM runs WHERE id = $1")
                .bind(follow)
                .fetch_one(&t.db.pool)
                .await
                .unwrap();
        assert_eq!(trigger, "answer");
        assert!(prompt.contains(crate::fence::ANSWER_BEGIN) && prompt.contains("yes — keep it"));
        assert!(list(&t.db, ws, false).await.unwrap().is_empty());
        let again = crate::asks::answer(&orch, q, "no").await.unwrap_err();
        assert!(matches!(again, crate::approvals::Refusal::Conflict(_)));
        t.finish().await;
    }

    /// A proposal is decided once, and must name things in its own workspace.
    #[tokio::test]
    async fn a_proposal_is_decided_once_and_stays_in_its_workspace() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, project) = t.project(dir.path(), true).await;
        let (_, elsewhere) = t.project(&dir.path().join("o"), true).await;
        let mine = t.card(project, "mine").await;
        let theirs = t.card(elsewhere, "theirs").await;
        use crate::decisions::{claim, propose, Effect};
        let foreign = propose(
            &t.db,
            ws,
            None,
            None,
            "Ada",
            &Effect::StartCard { card_id: theirs },
            "go",
        )
        .await;
        assert!(
            foreign.is_err(),
            "another workspace's card is not this agent's to propose"
        );
        let ghost = propose(
            &t.db,
            ws,
            None,
            None,
            "Ada",
            &Effect::PauseAgent {
                agent: "Nobody".into(),
            },
            "go",
        )
        .await;
        assert!(ghost.is_err());
        let id = propose(
            &t.db,
            ws,
            None,
            None,
            "Ada",
            &Effect::StartCard { card_id: mine },
            "ready",
        )
        .await
        .unwrap();
        assert_eq!(
            claim(&t.db, id, "approved").await.unwrap(),
            Some(Effect::StartCard { card_id: mine })
        );
        assert_eq!(
            claim(&t.db, id, "denied").await.unwrap(),
            None,
            "the second click finds it decided"
        );
        t.finish().await;
    }
}
