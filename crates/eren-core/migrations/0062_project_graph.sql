-- What the files contain, and what depends on what.
--
-- The vector index answers "where is the code that does X". This answers the
-- other two questions a person opening an unfamiliar repository asks: what is
-- in here, and what would I break. Both are read out of the source by a real
-- parser, so a definition named here exists at the line named here.

-- One row per file already exists — `project_documents` is keyed on
-- (project_id, rel_path), which is the grain a file table wants. A second
-- table listing the same files would drift from it, and 0061 renamed rather
-- than duplicated for exactly this reason. Two columns rather than a copy.
--
-- `lang` is the parser's verdict, not the extension's: it stays NULL for a
-- file no grammar here can read, which is how the graph knows to draw it
-- without claiming to know its insides.
ALTER TABLE project_documents ADD COLUMN lang TEXT;
-- Importance, 0..1, from a PageRank over the import edges. Deliberately used
-- for node size and as a tiebreaker only — measured on this repository it puts
-- `api.ts` and `db.rs` on top, which are the files somebody already knows
-- about. It describes what the code leans on, not what you were looking for.
ALTER TABLE project_documents ADD COLUMN rank REAL NOT NULL DEFAULT 0;

-- What a file defines.
--
-- Rows are replaced wholesale when a file's hash changes, never patched: a
-- half-updated symbol set would name a function at a line it has moved away
-- from, and being sent to the wrong line is worse than being sent nowhere.
CREATE TABLE project_symbols (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Denormalized so a project-wide symbol lookup is one indexed scan.
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    document_id UUID NOT NULL REFERENCES project_documents(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    -- function | class | struct | enum | trait | impl | type | const |
    -- component | method. A closed set, per language, mapped from the
    -- grammar's node kinds — never the raw node kind, which differs between
    -- grammars for the same idea.
    kind        TEXT NOT NULL,
    -- 1-based, matching what an editor shows and what `chunk.start_line`
    -- already means.
    line        INT NOT NULL,
    -- The first line of the declaration, trimmed and capped. Enough to tell
    -- two overloads apart in a list without storing the body.
    signature   TEXT
);
CREATE INDEX project_symbols_lookup ON project_symbols (project_id, name);
CREATE INDEX project_symbols_document ON project_symbols (document_id);

-- What a file asks for, exactly as written.
--
-- Stored unresolved on purpose. Resolution depends on the whole file set —
-- adding one file can make a previously dangling specifier point somewhere —
-- so it is redone from these rows on every pass rather than frozen at the
-- moment the file was parsed.
CREATE TABLE project_imports (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    document_id UUID NOT NULL REFERENCES project_documents(id) ON DELETE CASCADE,
    -- "../lib/api", "crate::db", "django.db.models" — verbatim.
    specifier   TEXT NOT NULL,
    line        INT NOT NULL
);
CREATE INDEX project_imports_document ON project_imports (document_id);
CREATE INDEX project_imports_project ON project_imports (project_id);

-- The resolved edges, recomputed whole on every pass.
--
-- Only edges whose both ends are files in this project: an import of `react`
-- resolves to nothing here and is dropped rather than drawn as a node that
-- does not exist. `weight` is how many specifiers in the source file resolve
-- to the target, so a folded edge can say how much is behind it instead of
-- implying one.
CREATE TABLE project_edges (
    project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    from_document UUID NOT NULL REFERENCES project_documents(id) ON DELETE CASCADE,
    to_document   UUID NOT NULL REFERENCES project_documents(id) ON DELETE CASCADE,
    weight        INT NOT NULL DEFAULT 1,
    PRIMARY KEY (from_document, to_document)
);
CREATE INDEX project_edges_project ON project_edges (project_id);
CREATE INDEX project_edges_to ON project_edges (to_document);

-- How much of what was asked for was found.
--
-- Surfaced rather than swallowed, because the failure mode of a dependency
-- graph is silent: a specifier that resolves to nothing draws no edge, and a
-- node with no edges reads as "nothing depends on this, safe to delete". A
-- reader who can see "1,204 of 1,318 imports resolved" knows how much to trust
-- an empty neighbourhood; a reader who cannot see it has no way to find out.
ALTER TABLE project_index ADD COLUMN edges_total INT NOT NULL DEFAULT 0;
ALTER TABLE project_index ADD COLUMN imports_total INT NOT NULL DEFAULT 0;
ALTER TABLE project_index ADD COLUMN imports_resolved INT NOT NULL DEFAULT 0;
ALTER TABLE project_index ADD COLUMN symbols_total INT NOT NULL DEFAULT 0;

-- Which parser produced what is stored.
--
-- The hash-diff answers "did this file change", which is the wrong question
-- when what changed is the *reader*. Teaching the extractor a new node kind
-- leaves every hash identical and every stored symbol list a version behind,
-- and the index would sit there stale and confident. A bumped version re-reads
-- everything once — the same trick `rag::embed::MODEL_TAG` plays on vectors.
-- Starts at 0, which no released parser claims, so the first pass after this
-- migration re-reads a project that was indexed before symbols existed.
ALTER TABLE project_index ADD COLUMN parse_version INT NOT NULL DEFAULT 0;
