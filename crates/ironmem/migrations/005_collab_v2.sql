-- Collab v2: post-PlanLocked coding loop.
--
-- Extends collab_sessions with the columns needed to track the per-task
-- 5-phase debate, the 2-pass global review, branch drift checks, and the
-- PR handoff. The session that reached PlanLocked in v1 is reused here — no
-- new row is created for the coding phase.
--
-- All new columns are nullable / default-zero so existing v1 sessions (which
-- never leave PlanLocked) remain valid after this migration.
ALTER TABLE collab_sessions ADD COLUMN task_list           TEXT;
ALTER TABLE collab_sessions ADD COLUMN current_task_index  INTEGER;
ALTER TABLE collab_sessions ADD COLUMN task_review_round   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE collab_sessions ADD COLUMN global_review_round INTEGER NOT NULL DEFAULT 0;
ALTER TABLE collab_sessions ADD COLUMN base_sha            TEXT;
ALTER TABLE collab_sessions ADD COLUMN last_head_sha       TEXT;
ALTER TABLE collab_sessions ADD COLUMN pr_url              TEXT;
ALTER TABLE collab_sessions ADD COLUMN coding_failure      TEXT;

INSERT OR IGNORE INTO schema_version (version) VALUES (5);
