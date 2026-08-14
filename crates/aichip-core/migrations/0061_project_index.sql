-- The semantic index grows from spaces to repositories.
--
-- These two tables were never space-specific: both are keyed on `project_id`,
-- and `retrieve.rs` already filters on it alone. A parallel `project_chunks`
-- would have been a near-copy with a second retrieval path to keep in step,
-- so the tables are renamed rather than duplicated and a repository is simply
-- another project whose files get indexed.
ALTER TABLE space_documents RENAME TO project_documents;
ALTER TABLE space_chunks RENAME TO project_chunks;
ALTER INDEX space_documents_project RENAME TO project_documents_project;
ALTER INDEX space_chunks_project RENAME TO project_chunks_project;

-- Where a chunk sits, so a hit can say "api.ts:412" and name the function it
-- landed in rather than handing back an anonymous paragraph. NULL for the
-- document formats that have no lines to count (pdf, xlsx).
ALTER TABLE project_chunks ADD COLUMN start_line INT;
ALTER TABLE project_chunks ADD COLUMN symbol TEXT;

-- Ranking reads every chunk for one project filtered by model, and does it on
-- every query. At a space's handful of documents that was a small scan; a
-- repository is thousands of chunks, so the filter earns an index of its own.
CREATE INDEX project_chunks_ranking ON project_chunks (project_id, embedding_model);

-- What the index knows about itself, one row per project.
--
-- Two phases, deliberately separate: `structure` is a parse and finishes in
-- seconds, `embedding` waits on a 35MB model download the first time. Folding
-- them into one "indexing" state would make the UI claim the map is
-- incomplete long after it is finished and correct.
CREATE TABLE project_index (
    project_id UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    -- never | structure | embedding | ready | failed
    phase      TEXT NOT NULL DEFAULT 'never',
    -- The commit the current index was read at. Compared against a live
    -- `git rev-parse HEAD` to decide whether anything needs re-reading, and
    -- rendered so a map can say which tree it describes — a card's run happens
    -- in a worktree on another branch, and an unqualified map would quietly
    -- describe the wrong one.
    head_sha   TEXT,
    -- Bumped only when files or edges actually change. The dashboard refetches
    -- the graph on a change and ignores the poll otherwise, so a canvas never
    -- re-lays out under the cursor while embeddings are still filling in.
    structure_version BIGINT NOT NULL DEFAULT 0,
    files_total   INT NOT NULL DEFAULT 0,
    files_parsed  INT NOT NULL DEFAULT 0,
    files_embedded INT NOT NULL DEFAULT 0,
    -- Why the last attempt failed, when `phase` is 'failed'. The last good
    -- index stays queryable underneath it — a failed refresh must not blank a
    -- working map.
    error      TEXT,
    -- There was nothing to read: a folder with no file in a language this
    -- understands. A state, not an error, and the UI says so in a sentence.
    note       TEXT,
    indexed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
