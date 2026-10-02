-- A page's optimistic-concurrency token, separate from `current_seq`.
--
-- `current_seq` was doing this job and could not do it. Autosaves inside the
-- coalescing window extend the current revision *in place*, so the pointer does
-- not move: two editors that both loaded revision 5 both still matched 5 after
-- the first one saved, so the second was accepted as though nothing had
-- happened. One person's paragraph was replaced by the other's with no
-- conflict, no banner, and — because the coalesce overwrote the row rather than
-- appending — no history entry to recover it from.
--
-- This counter advances on *every* accepted body write, coalesced or not, which
-- is the property `current_seq` deliberately does not have.
ALTER TABLE kb_articles
    ADD COLUMN body_version bigint NOT NULL DEFAULT 0;

-- Start existing pages at their revision pointer rather than at zero. Nothing
-- depends on the two numbers agreeing — they diverge the first time a save
-- coalesces — but a page whose history shows five revisions reading
-- `body_version = 5` is far easier to reason about in psql than one reading 0.
UPDATE kb_articles SET body_version = current_seq;
