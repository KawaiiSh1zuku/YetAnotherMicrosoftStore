CREATE TABLE products (
    product_id TEXT PRIMARY KEY NOT NULL,
    package_family_name TEXT,
    app_name TEXT,
    publisher TEXT,
    market TEXT NOT NULL,
    languages_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE package_versions (
    update_id TEXT PRIMARY KEY NOT NULL,
    product_id TEXT NOT NULL REFERENCES products(product_id) ON DELETE CASCADE,
    package_family_name TEXT,
    package_moniker TEXT NOT NULL,
    identity_name TEXT,
    publisher TEXT,
    resource_id TEXT,
    package_kind TEXT NOT NULL DEFAULT 'unknown',
    version TEXT NOT NULL,
    architecture TEXT NOT NULL,
    language TEXT,
    market TEXT NOT NULL,
    format TEXT NOT NULL,
    minimum_os_version TEXT,
    is_neutral INTEGER,
    content_id TEXT,
    file_size INTEGER,
    sha256 TEXT,
    install_source TEXT NOT NULL
);

CREATE INDEX package_versions_product_idx
    ON package_versions(product_id, version);
CREATE INDEX package_versions_identity_idx
    ON package_versions(package_family_name, architecture, language);
CREATE INDEX package_versions_applicability_idx
    ON package_versions(product_id, package_kind, architecture, language, market);
CREATE INDEX package_versions_content_idx
    ON package_versions(content_id);

CREATE TABLE package_dependencies (
    source_update_id TEXT NOT NULL REFERENCES package_versions(update_id) ON DELETE CASCADE,
    target_update_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    PRIMARY KEY (source_update_id, target_update_id, kind)
);

CREATE TABLE package_associations (
    package_family_name TEXT PRIMARY KEY NOT NULL,
    product_id TEXT,
    content_id TEXT,
    identity_name TEXT NOT NULL,
    publisher TEXT NOT NULL,
    confidence TEXT NOT NULL,
    observed_at INTEGER NOT NULL
);

CREATE INDEX package_associations_product_idx
    ON package_associations(product_id, confidence);
CREATE INDEX package_associations_identity_idx
    ON package_associations(identity_name, publisher);

CREATE TABLE jobs (
    job_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    product_id TEXT NOT NULL,
    requested_market TEXT NOT NULL,
    requested_architectures_json TEXT NOT NULL,
    requested_languages_json TEXT NOT NULL,
    deployment_scope TEXT NOT NULL,
    selected_update_id TEXT,
    package_family_name TEXT,
    stage TEXT NOT NULL,
    bytes_done INTEGER NOT NULL,
    bytes_total INTEGER,
    deployment_progress INTEGER
        CHECK (deployment_progress IS NULL OR deployment_progress BETWEEN 0 AND 100),
    version TEXT,
    architecture TEXT,
    language TEXT,
    error_json TEXT,
    event_sequence INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX jobs_stage_updated_idx ON jobs(stage, updated_at);
CREATE INDEX jobs_package_family_idx ON jobs(package_family_name);

CREATE TABLE job_events (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'created', 'imported', 'stage_changed', 'progress_recorded',
        'deployment_progress_recorded', 'selection_recorded', 'failed',
        'completed', 'cancelled', 'recovered'
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

CREATE TABLE cache_entries (
    cache_key TEXT PRIMARY KEY NOT NULL,
    job_id TEXT,
    update_id TEXT NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    state TEXT NOT NULL,
    last_accessed_at INTEGER NOT NULL
);

CREATE INDEX cache_entries_eviction_idx
    ON cache_entries(state, last_accessed_at);
CREATE INDEX cache_entries_job_idx ON cache_entries(job_id);

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE install_observations (
    package_family_name TEXT PRIMARY KEY NOT NULL,
    product_id TEXT,
    source TEXT NOT NULL,
    observed_at INTEGER NOT NULL
);

CREATE INDEX install_observations_product_idx
    ON install_observations(product_id);

CREATE TABLE diagnostics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT,
    code TEXT NOT NULL,
    stage TEXT,
    operation TEXT NOT NULL,
    os_error_code INTEGER,
    retryable INTEGER NOT NULL,
    occurred_at INTEGER NOT NULL
);

CREATE INDEX diagnostics_code_time_idx ON diagnostics(code, occurred_at);
CREATE INDEX diagnostics_job_time_idx ON diagnostics(job_id, occurred_at);
