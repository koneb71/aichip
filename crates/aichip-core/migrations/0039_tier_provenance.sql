-- Which tier a run used, who chose it, and why.
--
-- aichip refuses to make a consequential choice on the user's behalf without
-- saying so — it is why starting a Reviewed card on an engine that cannot ask
-- is refused outright rather than quietly downgraded to auto-edit. Automatic
-- tier routing is the same shape of decision: it changes which model runs the
-- work, and it happens without anyone clicking anything.
--
-- So the choice is recorded before the process starts, not derived afterwards.
-- `model` on its own would not answer it: two tiers can map to one model, and
-- the mapping is editable, so a model id read back next week cannot say which
-- tier asked for it or whether a person or the router picked it.
ALTER TABLE runs ADD COLUMN tier_resolved TEXT;
-- variant | agent | card | auto — where the tier came from.
ALTER TABLE runs ADD COLUMN tier_source   TEXT;
-- The rule that fired, stable and groupable, so the spend view can show that
-- one rule is routing badly rather than leaving it to be noticed card by card.
ALTER TABLE runs ADD COLUMN tier_rule     TEXT;
-- The sentence shown on the card.
ALTER TABLE runs ADD COLUMN tier_reason   TEXT;

-- Steps resolve their own model in workflow and organization runs, so they
-- need the same four or "cost by tier" stays a lie for the runs that cost the
-- most.
ALTER TABLE steps ADD COLUMN tier_resolved TEXT;
ALTER TABLE steps ADD COLUMN tier_source   TEXT;
ALTER TABLE steps ADD COLUMN tier_rule     TEXT;
ALTER TABLE steps ADD COLUMN tier_reason   TEXT;
