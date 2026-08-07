-- Which GitHub repository a project is, when it is one.
--
-- Until now a project's only identity was its local path, so every GitHub
-- feature re-derived the repository by shelling out to git — on every drawer
-- render and every poll tick. This is that answer, kept.
--
-- One column holding `owner/repo` rather than two holding the halves, because
-- that is `gh`'s own addressing format (`-R owner/repo`): every consumer wants
-- it joined, and splitting it means every read site puts it back together. It
-- also already has room for `HOST/OWNER/REPO`, which is what `gh` uses for an
-- enterprise host, if that ever arrives.
--
-- Deliberately NOT unique. Two projects being two clones of the same
-- repository is a normal thing to want — one to work in, one to compare
-- against — and a constraint here would refuse the second with an error about
-- a column nobody set.
ALTER TABLE projects ADD COLUMN github_repo TEXT;

CREATE INDEX IF NOT EXISTS projects_with_github_repo
    ON projects (github_repo)
    WHERE github_repo IS NOT NULL;
