//! Agent memory: what an agent remembers about the work it has done.
//!
//! Memories are written automatically — when an agent finishes a task, or
//! answers an @-mention on a card — and recalled into that agent's next runs.
//! They are deliberately compact summaries, not transcripts: the goal is
//! continuity ("you refactored the auth middleware yesterday"), not archival.

use crate::db::Db;
use uuid::Uuid;

/// Hard cap per memory row. Anything longer stops being a memory and starts
/// being a log; truncation is by characters so multibyte text can't panic.
const MAX_MEMORY_CHARS: usize = 400;

/// How many memories a run gets to see. Recency beats completeness — an
/// agent's prompt should carry its last few days, not its life story.
const RECALL_LIMIT: i64 = 10;

#[derive(Debug, Clone)]
pub struct Memory {
    pub id: Uuid,
    pub kind: String,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Store one memory for an agent, clipped to size.
pub async fn remember(
    db: &Db,
    agent_id: Uuid,
    project_id: Option<Uuid>,
    task_id: Option<Uuid>,
    kind: &str,
    content: &str,
) -> anyhow::Result<()> {
    let content = clip(content);
    if content.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO agent_memories (agent_id, project_id, task_id, kind, content)
         VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(agent_id)
    .bind(project_id)
    .bind(task_id)
    .bind(kind)
    .bind(&content)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// The agent's most recent memories: this project's plus its global ones.
pub async fn recall(
    db: &Db,
    agent_id: Uuid,
    project_id: Option<Uuid>,
) -> anyhow::Result<Vec<Memory>> {
    let rows: Vec<(Uuid, String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, kind, content, created_at FROM agent_memories
         WHERE agent_id = $1 AND (project_id = $2 OR project_id IS NULL)
         ORDER BY created_at DESC LIMIT $3",
    )
    .bind(agent_id)
    .bind(project_id)
    .bind(RECALL_LIMIT)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, kind, content, created_at)| Memory { id, kind, content, created_at })
        .collect())
}

/// Render memories as a prompt block, oldest first so the narrative reads
/// forward. None when there is nothing to say — the caller appends nothing.
pub fn render(memories: &[Memory]) -> Option<String> {
    if memories.is_empty() {
        return None;
    }
    let mut block = String::from(
        "\n\nYour memory — recent work you have done in this workspace, oldest first. \
         Use it for continuity; trust the code over your memory when they disagree:\n",
    );
    for m in memories.iter().rev() {
        block.push_str(&format!("- [{}] {}\n", m.created_at.format("%b %-d"), m.content));
    }
    Some(block)
}

/// Character-safe clip with a whitespace-normalised body: memories render
/// inside prompt bullet lists, where embedded newlines break the list.
fn clip(content: &str) -> String {
    let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_MEMORY_CHARS {
        return flat;
    }
    let clipped: String = flat.chars().take(MAX_MEMORY_CHARS).collect();
    format!("{clipped}…")
}

#[cfg(test)]
mod tests {
    use super::{clip, render, Memory, MAX_MEMORY_CHARS};
    use uuid::Uuid;

    #[test]
    fn clip_flattens_whitespace_and_respects_char_boundaries() {
        assert_eq!(clip("did  a\nthing\n"), "did a thing");
        // Multibyte text must clip by characters, not bytes.
        let long = "日".repeat(MAX_MEMORY_CHARS + 50);
        let out = clip(&long);
        assert!(out.chars().count() <= MAX_MEMORY_CHARS + 1);
        assert!(out.ends_with('…'));
        assert_eq!(clip("   "), "");
    }

    #[test]
    fn render_is_oldest_first_and_absent_when_empty() {
        assert_eq!(render(&[]), None);
        let mk = |content: &str, day: u32| Memory {
            id: Uuid::new_v4(),
            kind: "note".into(),
            content: content.into(),
            created_at: chrono::DateTime::parse_from_rfc3339(&format!(
                "2026-07-{day:02}T00:00:00Z"
            ))
            .unwrap()
            .with_timezone(&chrono::Utc),
        };
        // recall() returns newest first; the rendered block must read forward.
        let block = render(&[mk("newest", 20), mk("oldest", 10)]).unwrap();
        let older = block.find("oldest").unwrap();
        let newer = block.find("newest").unwrap();
        assert!(older < newer, "block should read oldest → newest");
    }
}
