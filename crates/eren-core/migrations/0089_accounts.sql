-- Accounts: who is signed in, and whose workspace is whose.
--
-- Off until an admin exists. With no row in `users` Eren behaves exactly as
-- before — loopback passes, other machines present the access token — so a
-- desktop install never meets a login it did not ask for. `eren admin create`
-- writes the first row, claims every workspace nobody owns, and from then on
-- every request needs a session (see `eren_core::users`).
CREATE TABLE users (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username             TEXT NOT NULL CHECK (username ~ '^[a-z0-9._-]{3,32}$'),
    -- An argon2id PHC string. Never the password, never logged.
    password_hash        TEXT NOT NULL,
    is_admin             BOOLEAN NOT NULL DEFAULT FALSE,
    -- Set by an admin's reset: the temporary password works once, to choose
    -- a new one, and for nothing else.
    must_change_password BOOLEAN NOT NULL DEFAULT FALSE,
    disabled_at          TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_username ON users (username);
-- Exactly one admin, held by the database rather than by whoever remembered
-- to check: two `eren admin create` racing each other cannot both win.
CREATE UNIQUE INDEX users_one_admin ON users (is_admin) WHERE is_admin;

-- A browser's sign-in. The cookie carries 32 random bytes; only their SHA-256
-- is kept, so a copy of this table signs nobody in.
CREATE TABLE sessions (
    token_hash   BYTEA PRIMARY KEY,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL
);
CREATE INDEX sessions_user ON sessions (user_id);
CREATE INDEX sessions_expiry ON sessions (expires_at);

-- NULL until accounts are turned on: the first admin adopts every workspace
-- that has none, and every workspace made after that is made by somebody.
ALTER TABLE workspaces ADD COLUMN owner_id UUID REFERENCES users(id) ON DELETE CASCADE;
CREATE INDEX workspaces_owner ON workspaces (owner_id);

-- Who, when the actor is a signed-in person. 'api' stays for requests made
-- with accounts off, where there is nobody to name.
ALTER TABLE audit_log DROP CONSTRAINT audit_log_actor_kind_check;
ALTER TABLE audit_log ADD CONSTRAINT audit_log_actor_kind_check
    CHECK (actor_kind IN ('api','agent','system','user'));
ALTER TABLE audit_log ADD COLUMN actor_user_id UUID;
-- A person's actions are kept for good, as 'api' rows always were.
DROP INDEX audit_log_prunable;
CREATE INDEX audit_log_prunable ON audit_log (at) WHERE actor_kind NOT IN ('api','user');
