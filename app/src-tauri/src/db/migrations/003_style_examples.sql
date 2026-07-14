CREATE TABLE style_examples (
  id                  INTEGER PRIMARY KEY,
  dictation_id        INTEGER REFERENCES dictations(id) ON DELETE SET NULL,
  context_key         TEXT,
  target_bundle       TEXT,
  raw_text            TEXT NOT NULL,
  final_text          TEXT NOT NULL,
  edit_distance_ratio REAL,
  source              TEXT NOT NULL,
  pinned              INTEGER NOT NULL DEFAULT 0,
  use_count           INTEGER NOT NULL DEFAULT 0,
  last_used_at        INTEGER,
  created_at          INTEGER NOT NULL
);
CREATE INDEX idx_style_examples_bundle_context ON style_examples (target_bundle, context_key);
