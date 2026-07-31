-- Which shape the agent decided this project needs.
--
-- The column exists because the two are run completely differently: a
-- Dockerfile is one `docker build` and one container, a compose recipe goes
-- through the same port-stripping and namespacing as a stack found in the repo.
-- Guessing from the text at run time would work, but a stored answer is one a
-- person approved and one the UI can show before anything is built.
ALTER TABLE preview_recipes
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'dockerfile';
