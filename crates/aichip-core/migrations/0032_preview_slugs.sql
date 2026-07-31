-- A name that survives a rebuild.
--
-- The port does not: it is asked of the OS each time a preview starts, so a
-- rebuild or a wake hands back a different one and the tab you had open is
-- pointing at nothing — or worse, at whatever took the port.
--
-- A hostname also fixes something ports cannot. Cookies ignore ports, so two
-- previews published on 127.0.0.1 share one cookie jar: log into one and the
-- other sees the session. Distinct hostnames give each preview its own origin,
-- which is what a browser actually uses to keep them apart.
ALTER TABLE previews ADD COLUMN IF NOT EXISTS slug TEXT;

-- Unique among previews that could still answer to it. A stopped preview's
-- slug is free to be reused by the next build of the same card.
CREATE UNIQUE INDEX IF NOT EXISTS previews_slug_alive
    ON previews (slug)
    WHERE slug IS NOT NULL AND status IN ('building', 'running', 'idle');
