-- A global pause on starting new work.
--
-- The thing you want when you notice you're burning through a rolling rate
-- limit: stop the queue feeding new runs, without killing what is already
-- in flight. Stored rather than held in memory so a pause survives a
-- restart — waking up to a machine that quietly resumed spending would be
-- the opposite of the point.
--
-- `settings` is the existing key/value table from 0001.
INSERT INTO settings (key, value) VALUES ('queue_paused', 'false'::jsonb)
    ON CONFLICT (key) DO NOTHING;
