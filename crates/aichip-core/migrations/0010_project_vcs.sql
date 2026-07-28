-- Whether a project is under version control.
--
-- Adding a folder now initializes a repository when it can, because the
-- worktree is what keeps an agent out of your checkout and what makes the
-- diff/review/merge flow possible. But init cannot always happen — git may be
-- missing, the directory may be read-only, or it may sit inside another
-- repository where nesting would be a trap. Those projects still work; their
-- runs just happen in place and settle straight to done, since there is no
-- diff to review.
--
-- Existing rows are 'git' because the old create endpoint rejected anything
-- without a .git directory.
ALTER TABLE projects ADD COLUMN vcs TEXT NOT NULL DEFAULT 'git';

-- Why a project ended up without version control, shown in the UI so the
-- state doesn't look like a bug. NULL for normal git projects.
ALTER TABLE projects ADD COLUMN vcs_note TEXT;
