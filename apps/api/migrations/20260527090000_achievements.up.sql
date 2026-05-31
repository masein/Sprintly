-- ─────────────────────────────────────────────────────────────────────────────
-- M9 — achievements.
--
-- The catalog is data, not code: rule details live in the JSONB so we can
-- tweak thresholds without a schema migration. The Rust scanner reads
-- (code, rule) and runs a hard-coded query per `code`. The `rule` JSON is
-- documentation for humans + a few tunable knobs (counts, windows).
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE achievements (
    id           UUID PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL,
    icon         TEXT NOT NULL,
    rule         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE user_achievements (
    user_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    achievement_id UUID NOT NULL REFERENCES achievements(id) ON DELETE CASCADE,
    awarded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    context        JSONB NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (user_id, achievement_id)
);
CREATE INDEX user_achievements_user_idx
    ON user_achievements (user_id, awarded_at DESC);

-- Seed the catalog. Idempotent (ON CONFLICT DO NOTHING on the unique code).
INSERT INTO achievements (id, code, title, description, icon, rule) VALUES
  (gen_random_uuid(), 'BUG_SLAYER',
     'Bug Slayer',
     'Closed 50 bug-type tasks. Mosquitoes everywhere.',
     'bug',
     '{"target":50}'),
  (gen_random_uuid(), 'PR_WIZARD',
     'PR Wizard',
     '50 tasks moved to Done. Casting from the staff.',
     'sparkles',
     '{"target":50}'),
  (gen_random_uuid(), 'ESTIMATOR_SUPREME',
     'Estimator Supreme',
     '20 tasks where the estimate landed within 10% of actual time logged.',
     'target',
     '{"target":20, "tolerance":0.10}'),
  (gen_random_uuid(), 'WATCHER_IN_WHEAT_FIELD',
     'Watcher in the Wheat Field',
     'Watching 30 or more tasks. Vigilant.',
     'eye',
     '{"target":30}'),
  (gen_random_uuid(), 'COFFEE_ADDICT',
     'Coffee Addict',
     'Closed a time log after midnight 10 times. We see you.',
     'coffee',
     '{"target":10}'),
  (gen_random_uuid(), 'SPRINT_CLOSER',
     'Sprint Closer',
     'Marked the last task of a sprint as done.',
     'flag',
     '{}'),
  (gen_random_uuid(), 'RETRO_HERO',
     'Retro Hero',
     'Authored the top-voted note in a closed retro.',
     'trophy',
     '{}'),
  (gen_random_uuid(), 'RTFM',
     'RTFM',
     'Read your own docs page.',
     'book',
     '{}')
ON CONFLICT (code) DO NOTHING;
