-- Which knowledge-base pages a chat message was sent with.
--
-- The same shape as `chat_message_agents` and `chat_message_skills`, and for
-- the same reason: a page can be deleted, and a dangling id in a `UUID[]` on
-- the message would only be discovered later, as a turn that failed to build
-- its own prompt. The cascade removes the attachment when the page goes and
-- the turn simply behaves as though nothing had been attached.
--
-- Per *message* rather than per chat, which is a decision and not an
-- accident: a page belongs to the question it was attached to. Carrying it
-- forward would keep pasting a runbook into every later turn of a
-- conversation that has moved on — paying for it again each time, and
-- stacking N copies of "read this as background" into one context, which is
-- precisely how a framing stops being read as a framing.
CREATE TABLE chat_message_articles (
    message_id UUID NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    article_id UUID NOT NULL REFERENCES kb_articles(id) ON DELETE CASCADE,
    -- The order the person attached them in. That ordering is a statement
    -- about what matters most, and sorting by anything else throws it away —
    -- the same rule `kb::for_run` follows for a card's pages.
    position   INT NOT NULL,
    PRIMARY KEY (message_id, article_id)
);

CREATE INDEX chat_message_articles_message ON chat_message_articles (message_id, position);
