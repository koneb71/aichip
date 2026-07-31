-- Where the user's plan stands, as their own CLI reports it.
--
-- aichip does not ask Anthropic anything and holds no credential: this is the
-- `rate_limit_event` the CLI already prints on its own stdout, kept instead of
-- discarded. Same relationship as everything else here — read the binary's
-- output, never its configuration.
--
-- One row per limit, not a history: "how much of my week is left" is a current
-- fact, and a table of every ping would be thousands of rows a day saying the
-- same thing. When it matters is at the moment you are deciding whether to
-- start something.
CREATE TABLE IF NOT EXISTS usage_limits (
    engine      TEXT NOT NULL,
    -- The CLI's own vocabulary: five_hour, seven_day.
    limit_type  TEXT NOT NULL,
    -- allowed | warning | blocked.
    status      TEXT NOT NULL,
    resets_at   TIMESTAMPTZ,
    -- The plan has spilled into paid overage, which is worth saying out loud
    -- before the bill does.
    using_overage BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (engine, limit_type)
);
