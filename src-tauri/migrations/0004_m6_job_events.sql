ALTER TABLE jobs ADD COLUMN event_sequence INTEGER NOT NULL DEFAULT 0;

CREATE TABLE job_events (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'created', 'imported', 'stage_changed', 'progress_recorded',
        'selection_recorded', 'failed', 'completed', 'cancelled', 'recovered'
    )),
    payload_json TEXT NOT NULL,
    projection_json TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    UNIQUE (job_id, sequence)
);

CREATE INDEX job_events_job_cursor_idx ON job_events(job_id, cursor);

CREATE TABLE job_commands (
    command_id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
    control TEXT NOT NULL CHECK (control IN ('pause', 'resume', 'cancel')),
    expected_sequence INTEGER NOT NULL CHECK (expected_sequence >= 0),
    created_at INTEGER NOT NULL,
    processed_at INTEGER,
    outcome_json TEXT,
    CHECK ((processed_at IS NULL AND outcome_json IS NULL)
        OR (processed_at IS NOT NULL AND outcome_json IS NOT NULL))
);

CREATE INDEX job_commands_pending_idx
    ON job_commands(job_id, created_at, command_id) WHERE processed_at IS NULL;

CREATE TABLE worker_leases (
    lease_key TEXT PRIMARY KEY NOT NULL CHECK (lease_key = 'worker'),
    owner_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    expires_at INTEGER NOT NULL
);

CREATE INDEX worker_leases_expires_idx ON worker_leases(expires_at);

CREATE TABLE job_targets (
    job_id TEXT NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('main', 'dependency', 'resource')),
    update_id TEXT NOT NULL,
    identity_name TEXT NOT NULL,
    publisher TEXT NOT NULL,
    version TEXT NOT NULL,
    architecture TEXT NOT NULL,
    resource_id TEXT,
    package_kind TEXT NOT NULL,
    expected_size INTEGER NOT NULL CHECK (expected_size > 0),
    sha256 TEXT NOT NULL,
    PRIMARY KEY (job_id, role, update_id)
);
