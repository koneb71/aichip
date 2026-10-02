-- Knowledge base: documentation people write, or ask an agent to write.
--
-- Article bodies are HTML from a rich-text editor and live here rather than in
-- object storage: they need to be searchable, and transactional with the row
-- that owns them. Only the things pasted *into* an article — screenshots,
-- PDFs — go to MinIO, where a 4 MB image doesn't end up in every backup.
CREATE TABLE kb_articles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    -- Rendered HTML as the editor produced it. Sanitised on write, never on
    -- read: a stored string that is only safe when someone remembers to clean
    -- it is a stored XSS waiting for the one caller who forgets.
    content_html TEXT NOT NULL DEFAULT '',
    -- One line for the list view and for the picker that tags articles onto
    -- cards. Derived from the body when the author doesn't write one.
    summary TEXT NOT NULL DEFAULT '',
    tags TEXT[] NOT NULL DEFAULT '{}',
    -- 'draft' until someone has actually read it. An agent-written article
    -- starts as a draft on purpose — generated documentation nobody has looked
    -- at is a claim, not a reference.
    status TEXT NOT NULL DEFAULT 'draft',
    -- 'human' | 'agent'. Worth knowing when you're deciding how much to trust
    -- a page you didn't write.
    origin TEXT NOT NULL DEFAULT 'human',
    -- The run that generated it, when an agent did.
    source_run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX kb_articles_workspace ON kb_articles (workspace_id, updated_at DESC);

-- Full-text search over title and body. Generated rather than trigger-
-- maintained, so it can never drift from the row it describes.
ALTER TABLE kb_articles ADD COLUMN search tsvector
    GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(summary, '')), 'B')
    ) STORED;
CREATE INDEX kb_articles_search ON kb_articles USING GIN (search);

-- Files and images pasted into an article. The bytes live in MinIO; this is
-- the record of what they are and who owns them.
CREATE TABLE kb_assets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- Null while the editor is still open: an upload happens before the
    -- article is saved, and claiming it afterwards is what makes the sweep
    -- able to tell an abandoned paste from a live one.
    article_id UUID REFERENCES kb_articles(id) ON DELETE CASCADE,
    object_key TEXT NOT NULL UNIQUE,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX kb_assets_article ON kb_assets (article_id);

-- Articles tagged onto a card. The point isn't decoration: a linked article is
-- injected into the run's prompt, so tagging one is how you tell an agent
-- "read this before you touch anything".
CREATE TABLE task_articles (
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    article_id UUID NOT NULL REFERENCES kb_articles(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, article_id)
);

-- The same, for a single comment — so "@agent, see #runbook" reaches the reply
-- run without permanently attaching the article to the card.
CREATE TABLE comment_articles (
    comment_id UUID NOT NULL REFERENCES task_comments(id) ON DELETE CASCADE,
    article_id UUID NOT NULL REFERENCES kb_articles(id) ON DELETE CASCADE,
    PRIMARY KEY (comment_id, article_id)
);

-- A run that writes an article rather than code. `kb_article_id` is what tells
-- the executor where to put the result — and, when it's a rewrite, which
-- existing article to hand the agent as its starting point.
ALTER TABLE runs ADD COLUMN kb_article_id UUID REFERENCES kb_articles(id) ON DELETE CASCADE;
ALTER TABLE runs ADD COLUMN kb_brief TEXT;
ALTER TABLE runs ADD COLUMN kb_project_id UUID REFERENCES projects(id) ON DELETE CASCADE;
