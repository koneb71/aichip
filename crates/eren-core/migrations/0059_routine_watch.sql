-- The watch kind: a routine that checks a page on a schedule. The URL is a
-- column, not part of the prompt, so the card can show it, the editor can
-- validate it, and the firing can compose the real prompt around it.
ALTER TABLE routines ADD COLUMN url TEXT;
