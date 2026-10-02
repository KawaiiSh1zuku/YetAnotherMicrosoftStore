CREATE TABLE products (
    product_id TEXT PRIMARY KEY NOT NULL,
    package_family_name TEXT,
    title TEXT,
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
    version TEXT NOT NULL,
    architecture TEXT NOT NULL,
    language TEXT,
    market TEXT NOT NULL,
    format TEXT NOT NULL,
    file_size INTEGER,
    sha256 TEXT,
    install_source TEXT NOT NULL
);

CREATE INDEX package_versions_product_idx
    ON package_versions(product_id, version);
CREATE INDEX package_versions_identity_idx
    ON package_versions(package_family_name, architecture, language);

CREATE TABLE package_dependencies (
    source_update_id TEXT NOT NULL REFERENCES package_versions(update_id) ON DELETE CASCADE,
    target_update_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    PRIMARY KEY (source_update_id, target_update_id, kind)
);

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
    version TEXT,
    architecture TEXT,
    language TEXT,
    requires_elevation INTEGER NOT NULL,
    error_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX jobs_stage_updated_idx ON jobs(stage, updated_at);
CREATE INDEX jobs_package_family_idx ON jobs(package_family_name);

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
