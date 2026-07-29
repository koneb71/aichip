-- The knowledge base becomes a wiki: pages get a place, an address, and a
-- memory.
--
-- Three things were wrong and one was dangerous. Wrong: there was no
-- hierarchy, so forty pages were forty equal rectangles; there was no project
-- scoping, so repo B's card could be handed repo A's deploy runbook as context
-- that "outranks your assumptions"; and the search vector covered the title and
-- the first 200 characters of body, so a term inside a runbook returned nothing
-- while the UI said "Nothing matches that".
--
-- Dangerous: an agent revision did an unconditional UPDATE of title, body and
-- summary. It destroyed a human's page with no prior copy, no diff and no
-- trace — and because it never touched `status`, it republished its own draft
-- under a page someone had already vouched for. From here the live body is the
-- newest `accepted` row in kb_revisions, and an agent can only propose one.

-- ---------------------------------------------------------------- hierarchy

-- RESTRICT, not the house-default CASCADE, deliberately: a cascade deletes a
-- whole subtree from one click, and delete collects MinIO object keys for ONE
-- article — so every descendant's bytes would be orphaned with nothing left
-- naming them. Delete reparents children in Rust instead.
ALTER TABLE kb_articles ADD COLUMN IF NOT EXISTS parent_id UUID REFERENCES kb_articles(id) ON DELETE RESTRICT;

-- Sibling order, the same idiom tasks.position uses: seconds-since-epoch
-- preserves insertion order, and a dragged page takes the midpoint of its
-- neighbours. Renormalised in Rust when a gap gets too small to halve again.
ALTER TABLE kb_articles ADD COLUMN IF NOT EXISTS position DOUBLE PRECISION NOT NULL DEFAULT extract(epoch FROM now());
UPDATE kb_articles SET position = extract(epoch FROM created_at);

-- Decoration that pays for itself: an icon'd tree scans far faster than forty
-- identical rows.
ALTER TABLE kb_articles ADD COLUMN IF NOT EXISTS icon TEXT NOT NULL DEFAULT '';

-- ------------------------------------------------------------------ spaces

-- A space is a repository. NULL is the workspace-wide "General" space: notes
-- that aren't about any one repo have to live somewhere, and inventing a third
-- grouping noun above projects to hold them would cost more than a nullable
-- column. SET NULL rather than CASCADE — removing a repo from the dashboard
-- must not delete the documentation someone wrote about it.
ALTER TABLE kb_articles ADD COLUMN IF NOT EXISTS project_id UUID REFERENCES projects(id) ON DELETE SET NULL;

-- Generation already knew which repo it read and threw the answer away.
-- Recover it. Hand-written pages have no such signal and honestly stay NULL.
UPDATE kb_articles a
   SET project_id = r.kb_project_id
  FROM runs r
 WHERE r.id = a.source_run_id AND r.kb_project_id IS NOT NULL;

-- Every tree read is workspace, then space, then parent, then order. The
-- existing (workspace_id, updated_at DESC) index serves none of them.
CREATE INDEX kb_articles_tree ON kb_articles (workspace_id, project_id, parent_id, position);

-- ------------------------------------------------- search that searches text

-- The plain-text projection, written by kb::render::prepare on every body
-- write. One artefact with three consumers: the search vector, the revision
-- diff, and the text agents are handed. Computing those separately is how they
-- drift apart.
ALTER TABLE kb_articles ADD COLUMN IF NOT EXISTS content_text TEXT NOT NULL DEFAULT '';

-- Postgres cannot alter a generated column's expression, so the vector is
-- dropped and rebuilt. DROP COLUMN takes kb_articles_search with it — the new
-- index must be recreated under exactly that name, or the list query quietly
-- sequential-scans forever with no error to notice.
ALTER TABLE kb_articles DROP COLUMN search;

-- Zero producers and zero consumers since the day it was added, and a
-- TypeScript type that claimed otherwise. The tree plus space scoping does the
-- job a taxonomy was going to do. Dropped after `search` so the generated
-- column goes first.
ALTER TABLE kb_articles DROP COLUMN tags;

-- content_text, not content_html: indexing markup would make `div`, `href`,
-- `colspan` and `youtube` searchable words. Weight D, so a body hit never
-- outranks a title hit.
ALTER TABLE kb_articles ADD COLUMN search tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
    setweight(to_tsvector('english', coalesce(summary, '')), 'B') ||
    setweight(to_tsvector('english', coalesce(content_text, '')), 'D')
) STORED;
CREATE INDEX kb_articles_search ON kb_articles USING GIN (search);
-- content_text is NOT derivable in SQL — the only HTML stripper is Rust — so
-- existing rows index exactly as before until kb::backfill runs at boot.

-- --------------------------------------------------------- the revision log

-- Full snapshots, not deltas. At this scale the whole trail is tens of
-- megabytes, and a restore that reconstructs from a delta chain is a restore
-- you cannot trust — which defeats the point of keeping a trail at all.
CREATE TABLE kb_revisions (
    article_id UUID NOT NULL REFERENCES kb_articles(id) ON DELETE CASCADE,
    -- Per-article and strictly increasing. Allocated inside a transaction
    -- holding a row lock on kb_articles; see kb::revisions::record.
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'edit',          -- edit | agent | restore | import
    -- Only 'accepted' rows are ever the live body; an agent writes 'pending'.
    -- 'superseded' means a stale proposal was accepted as a copy at a higher
    -- seq, so current_seq stays monotonic and the trail records what happened.
    state TEXT NOT NULL DEFAULT 'accepted',     -- pending | accepted | discarded | superseded
    -- Provenance per revision rather than per page: a page a person wrote is
    -- not permanently rebadged by one agent pass. With no users table, this is
    -- the only authorship that exists.
    author_kind TEXT NOT NULL DEFAULT 'human',  -- human | agent
    title TEXT NOT NULL,
    content_html TEXT NOT NULL,
    -- Stored per revision because it is the diff target. Diffing content_html
    -- instead reports ~100% changed on a one-word edit, and a reviewer who
    -- can't read the diff rubber-stamps it.
    content_text TEXT NOT NULL,
    -- The accepted seq this was written against. When it no longer equals
    -- kb_articles.current_seq the page moved on underneath the author, and the
    -- review UI has to say so rather than merge silently.
    base_seq INTEGER,
    restored_from INTEGER,                      -- set on kind='restore'
    run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    -- Why it was discarded, or what the author was going for. Free text,
    -- because there is nobody to point a foreign key at.
    note TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at TIMESTAMPTZ,
    PRIMARY KEY (article_id, seq),
    CONSTRAINT kb_revisions_state_check CHECK (state IN ('pending','accepted','discarded','superseded')),
    CONSTRAINT kb_revisions_kind_check CHECK (kind IN ('edit','agent','restore','import')),
    CONSTRAINT kb_revisions_author_check CHECK (author_kind IN ('human','agent'))
);
-- The sidebar badge and the page banner both ask "is anything waiting on me".
CREATE INDEX kb_revisions_pending ON kb_revisions (article_id) WHERE state = 'pending';
CREATE INDEX kb_revisions_recent ON kb_revisions (article_id, seq DESC);

-- The live pointer and the optimistic-concurrency token in one column: the seq
-- of the newest accepted revision, and what a PATCH must match. 0 means
-- "queued for an agent, nothing written yet" — the honest replacement for the
-- magic summary string the old page polled on forever. Left at 0 here on
-- purpose; kb::backfill sets it when it writes the import revision, because
-- content_text cannot be computed in SQL.
ALTER TABLE kb_articles ADD COLUMN IF NOT EXISTS current_seq INTEGER NOT NULL DEFAULT 0;
-- Invariant maintained in Rust, not by a trigger (this schema has none):
--   current_seq = max(seq) where article_id = id and state = 'accepted'

-- ---------------------------------------------------------------- backlinks

-- Extracted inside the sanitise pass, never in a separate one, so this can
-- never name an href the sanitiser stripped — a "what links here" list citing
-- content no reader can see. Rebuilt wholesale on every body write.
CREATE TABLE kb_links (
    from_id UUID NOT NULL REFERENCES kb_articles(id) ON DELETE CASCADE,
    to_id UUID NOT NULL REFERENCES kb_articles(id) ON DELETE CASCADE,
    PRIMARY KEY (from_id, to_id)
);
-- Backlinks are the direction people read; the primary key only serves the
-- forward one.
CREATE INDEX kb_links_to ON kb_links (to_id);

-- ---------------------------------------------------------------- integrity

-- Both were bare TEXT with no CHECK, so a PATCH could store any string at all.
-- `status` stops being advisory the moment an agent's read access gates on it,
-- so it has to become a real domain first.
UPDATE kb_articles SET status = 'draft'  WHERE status NOT IN ('draft','published');
UPDATE kb_articles SET origin = 'human'  WHERE origin NOT IN ('human','agent');
ALTER TABLE kb_articles ADD CONSTRAINT kb_articles_status_check CHECK (status IN ('draft','published'));
ALTER TABLE kb_articles ADD CONSTRAINT kb_articles_origin_check CHECK (origin IN ('human','agent'));

-- Deleting a page must not erase the spend record of the run that wrote it.
-- 0024 made this CASCADE, which takes the run, its steps, its events and its
-- recorded cost along with the page.
ALTER TABLE runs DROP CONSTRAINT runs_kb_article_id_fkey;
ALTER TABLE runs ADD CONSTRAINT runs_kb_article_id_fkey
    FOREIGN KEY (kb_article_id) REFERENCES kb_articles(id) ON DELETE SET NULL;

-- ------------------------------------------------------------ asset hygiene

-- The partial index attachments has had since 0009 and kb_assets never got.
-- Without it there is nothing for a sweeper to scan, so every paste into an
-- editor that is then abandoned leaks a row and its bytes permanently.
CREATE INDEX kb_assets_unclaimed ON kb_assets (created_at) WHERE article_id IS NULL;
