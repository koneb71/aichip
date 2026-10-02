-- A follow-up run keeps its note in its own column.
--
-- "Ask to fix" on a line of the diff queued a run carrying `comment_id`, and
-- every reader of that column takes it to mean "a comment reply": the
-- dispatcher sent the run down the reply path (whose query needs an agent a
-- fix run never has, so it failed before starting), the comments thread showed
-- "an agent is replying…" for as long as it existed, spend filed it as a
-- mention, and resume refused it as not a task run.
--
-- `review_comment_id` says what it is — the note this run acts on — and
-- nothing reads it as anything else. ON DELETE SET NULL: deleting the note
-- must not delete the record of the work done about it.
ALTER TABLE runs ADD COLUMN review_comment_id UUID REFERENCES task_comments(id) ON DELETE SET NULL;

UPDATE runs
   SET review_comment_id = comment_id, comment_id = NULL
 WHERE trigger = 'review' AND task_id IS NOT NULL AND comment_id IS NOT NULL;
