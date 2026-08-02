-- What a person has actually let an app do.
--
-- A row per grant rather than an array on `apps`, for two reasons. Revoking
-- becomes a delete, which is harder to get subtly wrong than rewriting an
-- array. And `last_used_at` gives someone something to revoke *on*: "granted on
-- the 3rd, never used since" is the sentence that makes a permissions screen
-- worth opening, and it cannot be reconstructed after the fact.
--
-- Deliberately not the same thing as `apps.requested_scopes`, which is what the
-- manifest asks for. A rebuild that starts asking for more has to surface as a
-- question, and it can only do that if what was asked and what was allowed are
-- stored separately. Same rule as an unapproved preview recipe: text nobody
-- read never inherits an approval given to different text.
CREATE TABLE IF NOT EXISTS app_grants (
    app_id       UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    scope        TEXT NOT NULL,
    granted_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    PRIMARY KEY (app_id, scope)
);
