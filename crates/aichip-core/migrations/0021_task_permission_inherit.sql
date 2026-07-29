-- Let a card inherit the workspace permission default instead of pinning one.
--
-- `permission_mode` has been `NOT NULL DEFAULT 'reviewed'` since 0001, and
-- until very recently there was no UI to set it — so every card carries
-- 'reviewed' as an accident of the schema, not as anything a user chose.
-- That stored value then beat the workspace default at run time, which is why
-- switching the default to "don't ask" changed nothing for existing cards and
-- they carried on asking about every write.
--
-- NULL now means "inherit", resolved when the run starts rather than frozen
-- when the card was created — so changing the default takes effect on work
-- that is already sitting in the backlog.
ALTER TABLE tasks ALTER COLUMN permission_mode DROP NOT NULL;
ALTER TABLE tasks ALTER COLUMN permission_mode DROP DEFAULT;

-- Clear the value nobody picked. This is not overriding a decision: there was
-- no control capable of producing it. A card whose mode is set from here on
-- was set deliberately and keeps it.
UPDATE tasks SET permission_mode = NULL WHERE permission_mode = 'reviewed';
