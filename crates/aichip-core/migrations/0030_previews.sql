-- Run a task's branch and look at it.
--
-- The point is not "deploy this project" — you already run your projects, and
-- you already solve port conflicts yourself. The point is that a card sitting
-- in review is a *diff*, and reading a diff is a poor way to answer "does this
-- look right". One container per card, on its own port, answers it directly.
--
-- One preview per task, enforced by a partial unique index rather than by the
-- handler: two clicks on Preview a second apart would otherwise leave a stray
-- container that nothing points at and nothing cleans up.

CREATE TABLE IF NOT EXISTS previews (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id         UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- Denormalised so the sweeper can find a project's containers without a
    -- join, and so a row still says where it came from after the task is gone.
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- building -> running -> stopped, or -> failed. Never back.
    status          TEXT NOT NULL DEFAULT 'building',
    -- Docker's id. Null while building: the container does not exist yet, and
    -- storing the name instead would lie about what we can actually kill.
    container_id    TEXT,
    image           TEXT,
    -- What we published on, and what inside the container we published from.
    host_port       INTEGER,
    container_port  INTEGER,
    -- Whether the port above was read from an EXPOSE line or guessed. Stored
    -- rather than inferred from the number: `EXPOSE 80` is the single most
    -- common line in a Dockerfile, and inferring "assumed" from "80" would
    -- label every nginx image a guess.
    port_assumed    BOOLEAN NOT NULL DEFAULT FALSE,
    -- The worktree this was built from. Recorded because the run that made it
    -- may be deleted while the preview is still up.
    source_path     TEXT NOT NULL,
    -- Why it failed, in Docker's own words. The build log's tail, not a
    -- paraphrase — "build failed" helps nobody.
    error           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    stopped_at      TIMESTAMPTZ
);

-- "Alive" is building or running. A stopped or failed preview does not block
-- starting a new one, which is what makes Retry just work.
CREATE UNIQUE INDEX IF NOT EXISTS previews_one_alive_per_task
    ON previews (task_id)
    WHERE status IN ('building', 'running');

CREATE INDEX IF NOT EXISTS previews_alive
    ON previews (status)
    WHERE status IN ('building', 'running');
