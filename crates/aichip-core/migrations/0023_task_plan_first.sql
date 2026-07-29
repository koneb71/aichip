-- Plan-first cards: the agent writes down what it intends to do, and the run
-- parks until a person approves it.
--
-- Only the card's *intent* is new. The run-level columns this needs —
-- `plan_approval` and `plan_approved_at` — already exist from 0012, where
-- organizations grew the same gate; nothing about them was org-specific.
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS plan_first BOOLEAN NOT NULL DEFAULT FALSE;

-- Whether a person rewrote the plan, rather than merely whether the text
-- differs. The work pass resumes the planning session, so the agent remembers
-- proposing something else — it has to be told which version is authoritative.
ALTER TABLE runs ADD COLUMN IF NOT EXISTS plan_edited BOOLEAN NOT NULL DEFAULT FALSE;

-- Feedback for the next planning pass, cleared once it has been used. Null
-- means "no outstanding revision request", which is what makes a re-queued
-- run plan afresh rather than replay old feedback forever.
ALTER TABLE runs ADD COLUMN IF NOT EXISTS plan_note TEXT;
