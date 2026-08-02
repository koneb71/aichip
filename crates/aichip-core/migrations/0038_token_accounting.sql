-- What a run actually cost, in tokens rather than only in dollars.
--
-- Both adapters have always parsed the cache counters off the CLI's own
-- stdout (`cache_read_input_tokens`, `cache_creation_input_tokens`) and the
-- persist step has always dropped them. That is the one number worth having:
-- cached input is a fraction of the price of fresh input, so cache hit rate,
-- not token count, is what separates an expensive run from a cheap one.
--
-- Same relationship to the engine as everything else here — keep what the
-- binary printed, ask nothing, and store no price table of our own. A
-- `cost_usd` on these rows is a figure the CLI reported; where it is NULL the
-- honest answer is "unknown", never tokens multiplied by a rate we invented.
ALTER TABLE runs ADD COLUMN cache_read_tokens     BIGINT NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN cache_creation_tokens BIGINT NOT NULL DEFAULT 0;

-- These counters include mid-run telemetry that no final message ever
-- reconciled — true for a run that was cancelled or whose engine died.
--
-- Worth a column rather than inferring it from `status`, because the two come
-- apart: a run can fail *after* reporting its totals (exact), and a completed
-- run's engine can die before the final message (an estimate). Only the code
-- that did the reconciling knows which happened.
ALTER TABLE runs ADD COLUMN tokens_provisional BOOLEAN NOT NULL DEFAULT FALSE;

-- Per-step accounting, so attribution stops being a division.
--
-- Cost has only ever been recorded per run, which is why the activity view
-- splits a run's total evenly across its assignments and the dashboard has to
-- admit the figure is "close, not exact". A workflow or organization run also
-- resolves a *model per step*, and nothing recorded that at all — so "cost by
-- tier" would have been a lie for exactly the runs that cost the most.
ALTER TABLE steps ADD COLUMN cost_usd              DOUBLE PRECISION;
ALTER TABLE steps ADD COLUMN input_tokens          BIGINT NOT NULL DEFAULT 0;
ALTER TABLE steps ADD COLUMN output_tokens         BIGINT NOT NULL DEFAULT 0;
ALTER TABLE steps ADD COLUMN cache_read_tokens     BIGINT NOT NULL DEFAULT 0;
ALTER TABLE steps ADD COLUMN cache_creation_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE steps ADD COLUMN model                 TEXT;
ALTER TABLE steps ADD COLUMN model_tier            TEXT;

-- The spend views all ask "the last N days", and every one of them would
-- otherwise seq-scan the whole history to answer it.
CREATE INDEX IF NOT EXISTS runs_spend_window
    ON runs (created_at)
    WHERE cost_usd IS NOT NULL OR input_tokens > 0;
