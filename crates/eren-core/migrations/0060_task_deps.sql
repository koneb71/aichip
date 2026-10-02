-- A card can be blocked by other cards: B builds on A, so B must not start
-- until A's work has LANDED. The bar is 'done', not 'review', on purpose —
-- a blocker sitting in review has a diff nobody merged, so a dependent card
-- starting then would branch from main *without* the work it depends on and
-- build on air.
CREATE TABLE task_deps (
    task_id    UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    blocked_by UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, blocked_by),
    CHECK (task_id <> blocked_by)
);
-- The reverse direction ("what does this card block") is asked by the board
-- and by the unblock check when a card lands.
CREATE INDEX task_deps_blocker ON task_deps (blocked_by);
