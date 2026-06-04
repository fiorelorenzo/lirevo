CREATE TABLE dictations (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at     INTEGER NOT NULL,
  language       TEXT,
  stt_model      TEXT NOT NULL,
  audio_ms       INTEGER,
  raw_text       TEXT NOT NULL,
  stt_ms         INTEGER NOT NULL,
  llm_model      TEXT,
  cleaned_text   TEXT NOT NULL,
  clean_ms       INTEGER,
  cleanup_status TEXT NOT NULL,
  cleanup_error  TEXT,
  inject_method  TEXT NOT NULL,
  inject_ms      INTEGER,
  total_ms       INTEGER NOT NULL,
  target_app     TEXT,
  target_bundle  TEXT
);
CREATE INDEX idx_dictations_created_at ON dictations (created_at DESC);
