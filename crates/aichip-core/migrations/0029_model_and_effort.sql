-- Choosing the model and how hard it thinks, everywhere work is started.
--
-- Both were reachable only indirectly before. A card's tier could be picked
-- when it was created and never changed again; reasoning effort could only be
-- set by binding an agent that happened to carry one, which meant "think harder
-- about this one thing" required inventing an agent. Chat could do neither — it
-- hardcoded Medium and no effort at all, which is why its composer had a picker
-- for the engine and nothing else.
--
-- Nullable everywhere, and that is the whole design: NULL means *inherit*,
-- resolved when the run dispatches rather than frozen at create time. So
-- changing the machine default reaches work already sitting in the backlog,
-- exactly as `permission_mode` has done since 0021.
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS effort TEXT;

-- Chat had neither, hence a composer that could only choose the engine.
ALTER TABLE chats ADD COLUMN IF NOT EXISTS model_tier TEXT;
ALTER TABLE chats ADD COLUMN IF NOT EXISTS effort TEXT;

COMMENT ON COLUMN tasks.effort IS
    'Reasoning effort, or NULL to inherit. Precedence: bound agent, then this, then the default_effort setting.';
