-- What a card must clear before it may be merged, per project, and the
-- verdicts an agent reviewer gives along the way.
--
-- Merge stays a person's click. The policy decides what that click requires
-- (passing checks, an approving review, a green pull request) and can be
-- overridden only with a written note, which is recorded.
CREATE TABLE project_review_policy (
    project_id                 UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    require_checks             BOOLEAN NOT NULL DEFAULT FALSE,
    require_review             BOOLEAN NOT NULL DEFAULT FALSE,
    reviewer_agent_id          UUID REFERENCES agents(id) ON DELETE SET NULL,
    max_rounds                 INT NOT NULL DEFAULT 2 CHECK (max_rounds BETWEEN 1 AND 3),
    require_pr_green           BOOLEAN NOT NULL DEFAULT FALSE,
    -- A person's standing decision to run the project's checks after every
    -- run, not only after a Full Auto one. Checks execute code an agent may
    -- have edited; this is the consent the click otherwise gives.
    run_checks_after_every_run BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at                 TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE review_decisions (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id           UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    run_id            UUID REFERENCES runs(id) ON DELETE SET NULL,
    reviewer_agent_id UUID REFERENCES agents(id) ON DELETE SET NULL,
    round             INT NOT NULL,
    verdict           TEXT NOT NULL CHECK (verdict IN ('approve','request_changes')),
    -- False when the reviewer ended without a verdict: recorded as changes
    -- requested (fail closed) and the loop stops for a person.
    submitted         BOOLEAN NOT NULL DEFAULT TRUE,
    summary           TEXT NOT NULL DEFAULT '',
    notes             JSONB NOT NULL DEFAULT '[]',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX review_decisions_task ON review_decisions (task_id, created_at DESC);
CREATE UNIQUE INDEX review_decisions_one_per_run ON review_decisions (run_id) WHERE run_id IS NOT NULL;

-- Which round a review pass is. On the run, because the verdict is written
-- by the reviewer mid-run and must not have to work it out.
ALTER TABLE runs ADD COLUMN review_round INT;
