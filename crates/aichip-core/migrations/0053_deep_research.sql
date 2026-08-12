-- Deep research: ask a question about a project, get a cited report.
--
-- Its own table rather than a chat flavour, because the two age differently:
-- a chat is a conversation whose value is the back-and-forth, a research is a
-- document whose value is the report. The report is what gets saved to the
-- knowledge base, listed, and re-run — none of which a chat_messages row can
-- carry.
CREATE TABLE researches (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- A research is meaningless without its repository, so it goes with it —
    -- the same CASCADE kb_project_id carries.
    project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    question      TEXT NOT NULL,
    -- Filled from the report's own first heading on completion; the list view
    -- falls back to the question while it is empty.
    title         TEXT NOT NULL DEFAULT '',
    -- NULL until a run completes. "No report yet" and "the run failed" are
    -- both NULL plus the run's status — nothing partial is ever written.
    report_md     TEXT,
    -- The save-to-KB idempotency token: a second click returns this article
    -- instead of filing another. ON DELETE SET NULL, because deleting the
    -- article must not delete its source — and a NULLed link is exactly what
    -- makes the button clickable again.
    kb_article_id UUID REFERENCES kb_articles(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX researches_project ON researches (project_id, created_at DESC);

-- A new dispatch column, following the kb_* precedent. CASCADE rather than
-- SET NULL: a research's runs are its transcript, and an orphaned research
-- run would fall through `execute()` into the task arm and crash on a row
-- with no task.
ALTER TABLE runs ADD COLUMN research_id UUID REFERENCES researches(id) ON DELETE CASCADE;
CREATE INDEX runs_research ON runs (research_id);
