-- Where a card came from, when it did not come from somebody typing it.
--
-- A discriminator rather than "source_url IS NOT NULL", because what follows
-- from it is specific: `Closes #42` in a pull request body is GitHub's
-- convention, and the next importer must not inherit it by accident.
ALTER TABLE tasks ADD COLUMN source TEXT;

-- 'owner/repo#42'. The whole address, so a card knows which repository's issue
-- it is even after the project is renamed or re-cloned.
ALTER TABLE tasks ADD COLUMN source_ref TEXT;

-- Where a person goes to read the original.
ALTER TABLE tasks ADD COLUMN source_url TEXT;

-- The number on its own, despite living inside `source_ref` too.
--
-- The pull-request body needs it as an integer, and re-parsing 'owner/repo#42'
-- there would be a second parser that can disagree with the first. Cheaper to
-- store the answer than to keep two readers in step.
ALTER TABLE tasks ADD COLUMN source_number INTEGER;

-- Importing the same issue twice makes one card.
--
-- The partial-unique + `ON CONFLICT DO NOTHING` idiom the previews table
-- already uses: the second import loses rather than producing a duplicate
-- board. Scoped to the project because one upstream repository can legitimately
-- be cloned into two projects, and each deserves its own board.
--
-- It covers live rows only, so deleting a card makes its issue importable
-- again — which is the behaviour somebody who deleted a card by mistake
-- expects, and the same reason `previews_one_alive_per_task` lets Retry work.
CREATE UNIQUE INDEX IF NOT EXISTS tasks_one_card_per_issue
    ON tasks (project_id, source_ref)
    WHERE source = 'github_issue';
