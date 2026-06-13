-- F3: latest CI/CD check status per linked commit/PR. A provider check or
-- pipeline event carries a commit SHA; we stamp the matching git_links rows
-- (a PR's head SHA, or a commit link) with the neutral state so the card can
-- show a pass/fail/pending chip.
ALTER TABLE git_links ADD COLUMN check_state text
    CHECK (check_state IN ('pending', 'passed', 'failed'));
