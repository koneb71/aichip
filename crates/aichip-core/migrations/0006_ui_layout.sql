-- Canvas node positions. Kept out of the YAML so committed workflows stay
-- clean and portable; layout is a local viewing preference.
ALTER TABLE workflows ADD COLUMN ui_layout JSONB NOT NULL DEFAULT '{}';
