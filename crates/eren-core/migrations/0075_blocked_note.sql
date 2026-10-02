-- What an agent said stopped it, in its own words, while it was working on
-- the card. Cleared when the card next starts: a new attempt is a new answer.
ALTER TABLE tasks ADD COLUMN blocked_note TEXT;
