-- Per-person word metering for the free-tier 2,000-word monthly cap.
ALTER TABLE usage ADD COLUMN word_count INTEGER NOT NULL DEFAULT 0;

CREATE INDEX idx_usage_user_day ON usage (clerk_user_id, day);
