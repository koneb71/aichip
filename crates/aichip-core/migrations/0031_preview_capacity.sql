-- Previews you can forget about.
--
-- The failure this prevents is mundane and certain: you build a preview, read
-- the page, close the drawer, and go to lunch. Nothing stops it. Two GB and two
-- CPUs stay held for the rest of the day, and by Friday there are three of them
-- and the machine your editor runs on is out of memory.
--
-- Idle-stopping removes the *container* and keeps the *image*, which is what
-- makes waking one cost seconds rather than the minutes a rebuild costs. Disk
-- is then the thing that grows instead, and disk is both cheaper and something
-- a person can be shown and asked about.

ALTER TABLE previews ADD COLUMN IF NOT EXISTS last_seen_at TIMESTAMPTZ;

-- Whether the image is still on disk, so a stopped preview knows whether
-- starting it again is a wake or a rebuild — and so the disk figure counts
-- only what is actually there.
ALTER TABLE previews ADD COLUMN IF NOT EXISTS image_kept BOOLEAN NOT NULL DEFAULT FALSE;

-- 'idle' joins building/running/stopped/failed: stopped by us, on purpose,
-- with the image kept. Distinct from 'stopped' because the two offer different
-- next actions and "why is this not running" has different answers.
--
-- Existing rows keep their status; nothing is rewritten.

-- Waking needs to find the most recent image for a card quickly.
CREATE INDEX IF NOT EXISTS previews_wakeable
    ON previews (task_id, created_at DESC)
    WHERE image_kept;

-- The sweeper's query: alive, and last looked at a while ago.
CREATE INDEX IF NOT EXISTS previews_idle_sweep
    ON previews (last_seen_at)
    WHERE status = 'running';
