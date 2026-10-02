-- Budgets with a scope, a window and more than one way to be spent.
--
-- There was one budget: a machine-wide dollar cap per day, in `settings`. It
-- could not say "this project gets $20 a week" or "the nightly routine gets
-- 200k tokens", and an engine that reports no price (Codex) cost $0 against
-- it however much it ran. A policy names what it covers, over which calendar
-- window, and caps any of dollars, output tokens and runs — tokens being how
-- an unpriced engine's work is counted at all.
CREATE TABLE budget_policies (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name              TEXT NOT NULL,
    -- What it covers. `scope_id` names the workspace, project, agent, team or
    -- routine; a machine-wide policy has none.
    scope_kind        TEXT NOT NULL
        CHECK (scope_kind IN ('machine', 'workspace', 'project', 'agent', 'team', 'routine')),
    scope_id          UUID,
    -- Calendar-aligned (`date_trunc`), in the database's time zone — the same
    -- "today" the old daily cap used.
    window_kind       TEXT NOT NULL DEFAULT 'day' CHECK (window_kind IN ('day', 'week', 'month')),
    cap_usd           DOUBLE PRECISION CHECK (cap_usd > 0),
    cap_output_tokens BIGINT CHECK (cap_output_tokens > 0),
    cap_runs          INT CHECK (cap_runs > 0),
    warn_percent      INT NOT NULL DEFAULT 80 CHECK (warn_percent BETWEEN 1 AND 100),
    -- `hold`: nothing new starts until the window turns. `stop`: that, and a
    -- run in flight is stopped when it crosses a token cap (dollars are only
    -- known when a run ends, so they cannot stop one midway).
    on_exceed         TEXT NOT NULL DEFAULT 'hold' CHECK (on_exceed IN ('hold', 'stop')),
    -- A start whose forecast could go past this many dollars of what is left
    -- asks first. NULL: never asks.
    confirm_above_usd DOUBLE PRECISION CHECK (confirm_above_usd >= 0),
    enabled           BOOLEAN NOT NULL DEFAULT TRUE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((scope_kind = 'machine') = (scope_id IS NULL)),
    CHECK (cap_usd IS NOT NULL OR cap_output_tokens IS NOT NULL OR cap_runs IS NOT NULL)
);
CREATE INDEX budget_policies_scope ON budget_policies (scope_kind, scope_id) WHERE enabled;

-- What happened to a policy, window by window. `warn` and `exceeded` are
-- written once per window (the index below), which is what makes their
-- notifications fire once rather than on every check. `override` adds
-- headroom for the window it is written in.
CREATE TABLE budget_incidents (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id    UUID NOT NULL REFERENCES budget_policies(id) ON DELETE CASCADE,
    window_start TIMESTAMPTZ NOT NULL,
    kind         TEXT NOT NULL
        CHECK (kind IN ('warn', 'exceeded', 'override', 'read_error', 'forecast_ack')),
    usd          DOUBLE PRECISION,
    tokens       BIGINT,
    runs         INT,
    note         TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX budget_incidents_once ON budget_incidents (policy_id, window_start, kind)
    WHERE kind IN ('warn', 'exceeded');
CREATE INDEX budget_incidents_window ON budget_incidents (policy_id, window_start);

-- Why a queued run is not starting yet, and which policy is holding it, so a
-- held run says so and an override can let it go at once.
ALTER TABLE queue ADD COLUMN hold_reason TEXT;
ALTER TABLE queue ADD COLUMN held_by UUID REFERENCES budget_policies(id) ON DELETE SET NULL;

-- The old daily cap becomes the machine-wide daily policy it always was.
INSERT INTO budget_policies (name, scope_kind, window_kind, cap_usd)
SELECT 'Daily budget', 'machine', 'day', (value #>> '{}')::double precision
  FROM settings
 WHERE key = 'daily_budget_usd'
   AND jsonb_typeof(value) = 'number'
   AND (value #>> '{}')::double precision > 0;
DELETE FROM settings WHERE key = 'daily_budget_usd';
