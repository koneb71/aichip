-- What happened, by whom, append-only.
--
-- No foreign keys and no cascades, deliberately: the ledger outlives what it
-- describes. A deleted card's history is exactly the history someone goes
-- looking for. Rows are written only by `aichip_core::audit::record` (a test
-- refuses any other writer) and removed only by its retention prune.
CREATE TABLE audit_log (
    id           BIGSERIAL PRIMARY KEY,
    at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 'api': a request to the dashboard's API. aichip has no login, so this
    --        is "something on this machine, through the API" — honestly not
    --        "a person", which a local process could also be.
    -- 'agent': an agent's tool call; actor_run_id says which run.
    -- 'system': aichip itself — the scheduler, a sweep, an automatic step.
    actor_kind   TEXT NOT NULL CHECK (actor_kind IN ('api','agent','system')),
    actor_run_id UUID,
    action       TEXT NOT NULL,
    entity_kind  TEXT,
    entity_id    TEXT,
    summary      TEXT NOT NULL DEFAULT '',
    detail       JSONB NOT NULL DEFAULT '{}'
);
CREATE INDEX audit_log_at ON audit_log (at DESC);
CREATE INDEX audit_log_entity ON audit_log (entity_kind, entity_id, at DESC);
CREATE INDEX audit_log_prunable ON audit_log (at) WHERE actor_kind <> 'api';
