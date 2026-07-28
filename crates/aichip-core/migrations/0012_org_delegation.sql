-- Delegation that can be reviewed, revised, and resumed.
--
-- The plan used to live in a Vec on the executor's stack, with `steps` as a
-- write-only mirror. Moving the plan into the database is what makes a human
-- approval gate, mid-run re-planning, and crash recovery the same mechanism
-- instead of three features.

-- Pause after planning so a human can edit the assignments before anyone
-- starts. Opt-in per run: unattended runs still work.
ALTER TABLE runs ADD COLUMN plan_approval BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE runs ADD COLUMN plan_approved_at TIMESTAMPTZ;

-- Re-plan budget. On the run rather than in memory so a parked run that
-- resumes doesn't get a fresh allowance.
ALTER TABLE runs ADD COLUMN replans INTEGER NOT NULL DEFAULT 0;

-- Where an org run works. A parked run that resumes, and the manager
-- answering a mid-task question, both need to reach the same worktree.
ALTER TABLE runs ADD COLUMN worktree_path TEXT;

-- Assignments gain acceptance criteria, a size the validator can enforce,
-- an explicit order, provenance, and a retry counter.
ALTER TABLE steps ADD COLUMN done_when TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE steps ADD COLUMN size TEXT;
ALTER TABLE steps ADD COLUMN position DOUBLE PRECISION;
ALTER TABLE steps ADD COLUMN origin TEXT NOT NULL DEFAULT 'plan'; -- plan | replan | user
ALTER TABLE steps ADD COLUMN attempt INTEGER NOT NULL DEFAULT 1;

-- A re-dispatched run must never materialize an assignment twice. This is
-- the structural guard behind the resume path.
CREATE UNIQUE INDEX steps_run_key ON steps (run_id, step_key);

-- How hard an agent thinks. Separate from model_tier: tier picks the model,
-- effort picks the reasoning spent. NULL leaves the CLI's own default.
ALTER TABLE agents ADD COLUMN effort TEXT;

-- Effort floor for an organization's planning steps. Planning is one call
-- that determines the quality of everything after it, so it is worth more
-- thinking than the manager's ordinary setting — without forcing a bigger,
-- costlier model.
ALTER TABLE teams ADD COLUMN planning_effort TEXT NOT NULL DEFAULT 'high';

-- Existing rows ordered by start time; from here position carries the order
-- so a queued assignment no longer has to claim a start time it doesn't have.
UPDATE steps SET position = extract(epoch FROM started_at) WHERE started_at IS NOT NULL;
