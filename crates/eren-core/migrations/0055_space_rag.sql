-- Semantic retrieval over a space's documents.
--
-- Embeddings are computed locally (fastembed/ONNX — aichip never calls a
-- model API) and stored as raw f32 little-endian BYTEA, ranked by cosine in
-- Rust. Not pgvector, deliberately: the embedded Postgres this app manages
-- ships no extensions, and a document space is thousands of chunks — brute
-- force ranks that in milliseconds. `embedding_model` is the escape hatch:
-- adopting pgvector later is a column add plus a decode, never a re-ingest,
-- and a model swap makes old rows invisible to retrieval rather than
-- garbage-ranked (retrieval filters on the current tag; reconcile re-embeds).

CREATE TABLE space_documents (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- A sanitized basename. Spaces are managed folders; nested paths are
    -- walked but stored with their relative path from the space root.
    rel_path     TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    bytes        BIGINT NOT NULL,
    -- pending: seen, not yet embedded. indexed: searchable. failed: the
    -- embedder said why (shown in the rail; retried on the next reconcile).
    -- unsupported: a real state, not an error — a PDF or binary stays in the
    -- folder where the CLI's own Read can still open it; it just is not in
    -- the semantic index.
    status       TEXT NOT NULL DEFAULT 'pending',
    error        TEXT,
    indexed_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, rel_path)
);
CREATE INDEX space_documents_project ON space_documents (project_id);

CREATE TABLE space_chunks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id     UUID NOT NULL REFERENCES space_documents(id) ON DELETE CASCADE,
    -- Denormalized so retrieval is one indexed scan over the project's
    -- chunks, joining documents only for the file name.
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    chunk_index     INT NOT NULL,
    content         TEXT NOT NULL,
    embedding       BYTEA NOT NULL,
    embedding_model TEXT NOT NULL,
    UNIQUE (document_id, chunk_index)
);
CREATE INDEX space_chunks_project ON space_chunks (project_id);
