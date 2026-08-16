-- A skill that came from somewhere else.
--
-- `npx skills add owner/repo` installs an Agent Skill into the project: a
-- directory under `.agents/skills/<name>/` holding a SKILL.md, symlinked into
-- `.claude/skills/` so Claude Code reads it natively, plus whatever the skill
-- bundles — real ones ship `resources/deploy.sh` and `scripts/*.mjs`.
--
-- That installed folder is the thing the engine actually reads, and it is the
-- source of truth. What these columns add is a *mirror* in aichip's own
-- library, so an installed skill can be `@name`d in chat, bound to a card, and
-- carried to an engine that does not read the format. The mirror is a
-- convenience; the folder is the fact.
--
-- Which is why the hash is here. `skills-lock.json` records the installer's
-- own content hash per skill; storing it on the row is what lets a later sync
-- tell "the file changed underneath us" from "nothing has happened", without
-- diffing prose.
--
-- Every column is nullable and a hand-written skill leaves them all NULL,
-- behaving exactly as it does today. `source_repo IS NOT NULL` is the whole
-- test for "this one is a mirror".
ALTER TABLE skills ADD COLUMN source_repo TEXT;

-- The path inside that repository, so a repo shipping nine skills can say
-- which of them this row is.
ALTER TABLE skills ADD COLUMN source_path TEXT;

-- The installer's `computedHash` at the moment this row was mirrored.
ALTER TABLE skills ADD COLUMN source_hash TEXT;

-- Which project it was installed into. Skills are workspace-scoped and this
-- one is too — it is reachable from every project's cards, like any other —
-- but the *files* live in one checkout, so "where do I go to edit this" needs
-- an answer. SET NULL rather than CASCADE: unloading a project should not
-- silently delete a skill that other projects' cards are bound to.
ALTER TABLE skills ADD COLUMN source_project_id UUID REFERENCES projects(id) ON DELETE SET NULL;

ALTER TABLE skills ADD COLUMN installed_at TIMESTAMPTZ;

-- Finding the mirror for a skill the installer just wrote. Partial, because
-- the common case is a workspace of hand-written skills where every one of
-- these is NULL.
CREATE INDEX skills_installed ON skills (source_project_id, source_repo)
    WHERE source_repo IS NOT NULL;
