-- WeldSpeak initial schema.
--
-- Clerk owns users and organizations; this database stores only what Clerk
-- does not. Clerk IDs are therefore foreign keys to an external system and are
-- stored as opaque text with no local `users` or `orgs` table to join against.

-- Desktop installs that completed the device authorization flow.
CREATE TABLE devices (
  id            TEXT PRIMARY KEY,
  clerk_user_id TEXT NOT NULL,
  platform      TEXT NOT NULL,          -- 'macos' | 'windows'
  label         TEXT NOT NULL,          -- hostname, so users can tell devices apart
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  last_seen_at  TEXT
);
CREATE INDEX idx_devices_user ON devices (clerk_user_id);

-- Refresh tokens, stored as SHA-256 hashes so a database leak does not hand
-- over live credentials. Rotated on every use: `replaced_by` forms a chain, so
-- a replayed old token is detectable rather than merely invalid.
CREATE TABLE refresh_tokens (
  token_hash  TEXT PRIMARY KEY,
  device_id   TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
  clerk_user_id TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (datetime('now')),
  expires_at  TEXT NOT NULL,
  revoked_at  TEXT,
  replaced_by TEXT
);
CREATE INDEX idx_refresh_device ON refresh_tokens (device_id);
CREATE INDEX idx_refresh_user ON refresh_tokens (clerk_user_id);

-- Vocabulary boosts. `scope='user'` rows are private to their owner;
-- `scope='org'` rows are the shared team glossary, readable by every member
-- and writable only by an org admin. Exactly one owner column is set per row.
CREATE TABLE dictionary_terms (
  id            TEXT PRIMARY KEY,
  scope         TEXT NOT NULL CHECK (scope IN ('user', 'org')),
  clerk_user_id TEXT,
  clerk_org_id  TEXT,
  term          TEXT NOT NULL,
  sounds_like   TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  CHECK (
    (scope = 'user' AND clerk_user_id IS NOT NULL AND clerk_org_id IS NULL) OR
    (scope = 'org'  AND clerk_org_id  IS NOT NULL AND clerk_user_id IS NULL)
  )
);
CREATE INDEX idx_terms_user ON dictionary_terms (clerk_user_id) WHERE scope = 'user';
CREATE INDEX idx_terms_org ON dictionary_terms (clerk_org_id) WHERE scope = 'org';
-- Duplicate terms would inflate the keyterm list and waste prompt budget.
CREATE UNIQUE INDEX idx_terms_user_unique ON dictionary_terms (clerk_user_id, term) WHERE scope = 'user';
CREATE UNIQUE INDEX idx_terms_org_unique ON dictionary_terms (clerk_org_id, term) WHERE scope = 'org';

-- Dictation history. Only written when the active org permits retention; see
-- org_settings.retain_transcripts.
CREATE TABLE transcripts (
  id            TEXT PRIMARY KEY,
  clerk_user_id TEXT NOT NULL,
  clerk_org_id  TEXT,
  raw           TEXT NOT NULL,
  formatted     TEXT NOT NULL,
  duration_ms   INTEGER NOT NULL,
  app_name      TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_transcripts_user ON transcripts (clerk_user_id, created_at DESC);
CREATE INDEX idx_transcripts_org ON transcripts (clerk_org_id, created_at DESC);

-- Per-organization policy, set by an admin.
CREATE TABLE org_settings (
  clerk_org_id       TEXT PRIMARY KEY,
  retain_transcripts INTEGER NOT NULL DEFAULT 1,
  monthly_minute_cap INTEGER,             -- NULL means uncapped
  updated_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Metered audio, aggregated per day. Rolled up rather than per-dictation so
-- the quota check stays a single indexed read on the hot path.
CREATE TABLE usage (
  clerk_org_id  TEXT NOT NULL DEFAULT '',   -- '' for personal (no active org)
  clerk_user_id TEXT NOT NULL,
  day           TEXT NOT NULL,              -- 'YYYY-MM-DD', UTC
  audio_seconds INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (clerk_org_id, clerk_user_id, day)
);
CREATE INDEX idx_usage_org_day ON usage (clerk_org_id, day);
