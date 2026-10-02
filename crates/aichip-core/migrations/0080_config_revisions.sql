-- The row as it was before each change to an agent, team, routine, skill,
-- check list, budget or the attention setting — so a bad edit can be undone.
--
-- A snapshot of the *previous* state, taken by the entity's own update
-- handler just before it writes (the project Brain's revisions work the same
-- way). Restoring goes back through that handler, so it is validated, gated
-- and recorded like any edit, and is itself a revision.
CREATE TABLE config_revisions (
    id          BIGSERIAL PRIMARY KEY,
    entity_kind TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    snapshot    JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX config_revisions_entity ON config_revisions (entity_kind, entity_id, id DESC);
