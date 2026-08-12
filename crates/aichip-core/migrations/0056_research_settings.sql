-- What a research runs as: its model tier and reasoning effort.
--
-- On the research, not the run, the way chats carry theirs (0029): a re-run
-- means "ask again the same way", so the choice has to survive the run that
-- made it. NULL means the defaults — Complex with the operator's effort for
-- that tier, which is what every research ran as before this existed.
ALTER TABLE researches ADD COLUMN model_tier TEXT;
ALTER TABLE researches ADD COLUMN effort TEXT;
