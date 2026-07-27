-- A chat session id is only resumable by the engine that produced it.
ALTER TABLE chats ADD COLUMN session_engine TEXT;
UPDATE chats SET session_engine = 'mock' WHERE session_id LIKE 'mock-%';
UPDATE chats SET session_engine = 'claude-code' WHERE session_id IS NOT NULL AND session_engine IS NULL;
