-- When each limit window changed state, rather than only where it stands now.
--
-- `usage_limits` (0037) is one row per limit and upserts in place, which is the
-- right shape for "can I start something right now" and the wrong shape for
-- every other question. The moment a window turns over, the fact that it ever
-- blocked is gone — so "am I hitting the weekly wall every week, or was that
-- once" cannot be answered at all, and that is the question that changes what
-- somebody does about it.
--
-- Still not a row per ping. The CLI prints `rate_limit_event` continuously
-- while a run works, and 0037 was right that storing every one of them is
-- thousands of identical rows a day. This records *transitions*: a row is
-- written only when the status or the window changed, which is the same
-- information with none of the repetition. `usage::record` does that check.
CREATE TABLE IF NOT EXISTS usage_events (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine      TEXT NOT NULL,
    -- The CLI's own vocabulary: five_hour, seven_day.
    limit_type  TEXT NOT NULL,
    -- allowed | warning | blocked, as `LimitStatus::as_str` spells them.
    status      TEXT NOT NULL,
    -- What it was immediately before, so a row reads as a transition on its own
    -- without needing the row before it. NULL means this is the first time this
    -- limit was ever heard from.
    previous    TEXT,
    resets_at   TIMESTAMPTZ,
    using_overage BOOLEAN NOT NULL DEFAULT FALSE,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Every read of this table is "the recent history of this limit", newest first.
CREATE INDEX IF NOT EXISTS usage_events_recent
    ON usage_events (limit_type, observed_at DESC);
