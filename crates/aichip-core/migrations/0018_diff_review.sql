-- Review comments anchored to a line of the diff.
--
-- The board has a review column and a diff view, but feedback could only be
-- attached to the whole card. Saying "the null check on line 42 is wrong"
-- meant re-describing the code in prose and hoping the agent found it. These
-- three columns are what turn the review column into an actual review.
ALTER TABLE task_comments ADD COLUMN file_path TEXT;
ALTER TABLE task_comments ADD COLUMN line INTEGER;

-- The diff hunk as it looked when the comment was written.
--
-- Snapshotted rather than re-derived, because the fix run changes the very
-- diff the line number refers to: by the time anyone reads the comment back,
-- line 42 is a different line. The text is what makes the comment still
-- legible afterwards, and it is what the agent is actually shown.
ALTER TABLE task_comments ADD COLUMN hunk TEXT;

-- A prompt for one run only.
--
-- Task runs take their prompt from `tasks.prompt`, which is right for
-- "run this task" and wrong for "fix this one thing in the diff you just
-- produced". Overriding per run keeps the card's brief intact — re-running
-- the task later should do the original job, not the last review note.
ALTER TABLE runs ADD COLUMN prompt_override TEXT;
