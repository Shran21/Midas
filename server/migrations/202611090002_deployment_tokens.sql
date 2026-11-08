-- A deployment token may enroll several devices (for GPO or Intune); such a token always needs
-- an approval per device, so it can never be preapproved. A single-use token stays used once
-- `used_at` is set, so tokens issued before this column existed keep their meaning.
ALTER TABLE midas.enrollment_tokens
    ADD COLUMN max_uses integer NOT NULL DEFAULT 1 CHECK (max_uses BETWEEN 1 AND 1000),
    ADD COLUMN use_count integer NOT NULL DEFAULT 0,
    ADD CONSTRAINT enrollment_tokens_uses CHECK (use_count BETWEEN 0 AND max_uses),
    ADD CONSTRAINT enrollment_tokens_shared_review CHECK (max_uses = 1 OR NOT preapproved);
