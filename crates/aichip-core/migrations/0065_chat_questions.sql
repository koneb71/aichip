-- Clarifying questions: the assistant asks instead of guessing.
--
-- The failure this is for: a request that reads as one thing and means
-- another. Today the assistant either picks an interpretation and creates the
-- wrong card, or writes a paragraph of "did you mean A or B?" that the person
-- has to answer in prose. A question with options is cheaper for both — one
-- click instead of a sentence, and no chance of the answer being misread.
--
-- **Not a parked run.** The obvious design is a tool call that blocks until a
-- person answers, which is how the permission prompt works. A chat turn takes
-- a concurrency permit from the same queue as everything else, so a turn
-- parked on a question holds one of a small number of slots for however long
-- somebody takes to look — and `routes::chat::active_run` counts it as active,
-- so the conversation could not be used to answer in the meantime. The
-- question is therefore *recorded*, the turn ends, and answering is the next
-- message. The session resumes, so the assistant carries on where it was.
CREATE TABLE chat_questions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    chat_id    UUID NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    -- The turn that asked. Lets the thread put the question under the reply
    -- it belongs to rather than at the end.
    run_id     UUID REFERENCES runs(id) ON DELETE SET NULL,
    -- `[{ question, header, options: [{ label, description }], multiSelect }]`
    -- — up to four, validated on the way in. Stored whole rather than
    -- normalised into rows: nothing queries inside it, and a question is only
    -- ever read back as the thing it was asked as.
    questions  JSONB NOT NULL,
    -- What the person picked, once they have. NULL means still open, which is
    -- what decides whether the buttons are offered — the same rule a plan's
    -- `plan_outcome` follows, and for the same reason: a live button under an
    -- answered question invites the same turn twice.
    answer     JSONB,
    answered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX chat_questions_open ON chat_questions (chat_id, created_at)
    WHERE answered_at IS NULL;
