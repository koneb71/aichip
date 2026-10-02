-- Plan mode for the assistant chat: look before you leap, here too.
--
-- A card already has this (`tasks.plan_first`, 0023). The chat did not, and
-- the chat is where the expensive mistakes start: it can create cards, assign
-- agents and *start* them, and the first sign that it misunderstood is a run
-- that has already spent money on the wrong thing.
--
-- On the chat rather than on the message, like the model tier and the effort
-- beside it: plan mode is a mode you are in, not a property of one sentence.
-- Turned off again when a plan is approved, because carrying it out is exactly
-- the thing plan mode is for not doing.
ALTER TABLE chats ADD COLUMN plan_mode BOOLEAN NOT NULL DEFAULT FALSE;

-- Whether this reply is a plan awaiting an answer.
--
-- Recorded rather than inferred from the text: "is this a plan?" cannot be
-- read back out of prose, and the button that carries it out must not appear
-- under a message that merely happens to contain a numbered list. Set from the
-- run that produced it, which knew.
ALTER TABLE chat_messages ADD COLUMN is_plan BOOLEAN NOT NULL DEFAULT FALSE;

-- Answered, and how. NULL while a plan is still open; 'approved' once it has
-- been carried out, 'superseded' when a later plan replaced it.
--
-- Kept so the thread reads honestly on the way back: a plan the person acted
-- on and a plan they walked away from look identical without it, and an
-- Approve button under a plan from three turns ago is an invitation to run
-- something twice.
ALTER TABLE chat_messages ADD COLUMN plan_outcome TEXT;
