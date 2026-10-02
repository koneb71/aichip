-- Engines become plural.
--
-- Until now one engine existed, so "which engine" was a question nothing had
-- to answer. Three consequences of that assumption are corrected here.

-- 1. A team can prefer an engine. NULL means "whatever the caller picked",
--    which is how a team stays portable across machines that have different
--    CLIs installed.
ALTER TABLE teams ADD COLUMN IF NOT EXISTS engine TEXT;

-- 2. An agent can prefer an engine. The column has existed since 0001 and has
--    never been read; it was NOT NULL DEFAULT 'claude-code', which would mean
--    every existing agent silently pins Claude the moment it starts being
--    honoured. Since nothing could ever set it, every row holds the default —
--    so clearing it is a no-op today and inheritance tomorrow.
ALTER TABLE agents ALTER COLUMN engine DROP NOT NULL;
ALTER TABLE agents ALTER COLUMN engine DROP DEFAULT;
UPDATE agents SET engine = NULL WHERE engine = 'claude-code';

-- 3. A session id is only resumable by the engine that produced it. Migration
--    0003 established this for chats; runs and steps kept raw session ids,
--    which was safe only while there was one engine to hand them back to.
--    Handing an `ses_…` to Claude doesn't fail loudly — it starts over with
--    none of the context, which is the quiet kind of wrong.
ALTER TABLE runs ADD COLUMN IF NOT EXISTS session_engine TEXT;
ALTER TABLE steps ADD COLUMN IF NOT EXISTS session_engine TEXT;

UPDATE runs SET session_engine = engine WHERE session_id IS NOT NULL;
UPDATE steps s SET session_engine = r.engine
  FROM runs r WHERE r.id = s.run_id AND s.session_id IS NOT NULL;
