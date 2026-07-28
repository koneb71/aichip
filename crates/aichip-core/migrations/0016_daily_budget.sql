-- A ceiling on what the machine may spend in a day.
--
-- The cap is global rather than per-workspace on purpose: the constraint it
-- protects is a subscription rate limit attached to one login, and splitting
-- an account-wide limit across workspaces would let two of them cheerfully
-- exhaust it together.
--
-- No row means no cap, so there is nothing to seed here — the key is written
-- the first time a limit is set. This migration exists to record the
-- convention alongside `queue_paused` from 0014 rather than leaving it
-- implicit in application code.
--
-- Recovery is deliberately time-based: the guard compares against spend
-- since `date_trunc('day', now())`, so a day that hit its cap reopens at
-- midnight with no scheduler, no timer, and nothing to get stuck.
COMMENT ON TABLE settings IS
    'Global key/value settings. Known keys: queue_paused (bool), daily_budget_usd (number).';
