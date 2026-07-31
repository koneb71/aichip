-- A stack has no single container and no single image, so stopping one means
-- `docker compose down` against the file it came up with — which is a rewritten
-- copy under ~/.aichip, not the project's own. Both paths are recorded because
-- neither is derivable later: the branch may have moved on, and the rewrite is
-- keyed to a host port chosen at start time.
ALTER TABLE previews ADD COLUMN IF NOT EXISTS compose_file TEXT;
ALTER TABLE previews ADD COLUMN IF NOT EXISTS compose_dir TEXT;
