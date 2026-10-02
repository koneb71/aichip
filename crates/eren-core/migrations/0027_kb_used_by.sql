-- Reverse lookups for "which tasks use this page".
--
-- Both tables key on the task or comment first, because that is the direction
-- the board reads them: open a card, list its pages. Asking the opposite
-- question — open a page, list its cards — had no index at all and fell back to
-- a sequential scan of every reference in the install.
--
-- The page side is the direction that matters for keeping a wiki honest. A
-- runbook attached to eleven cards is load-bearing and you should know before
-- rewriting it; one attached to nothing has never been read by anybody.
CREATE INDEX task_articles_article ON task_articles (article_id);
CREATE INDEX comment_articles_article ON comment_articles (article_id);
