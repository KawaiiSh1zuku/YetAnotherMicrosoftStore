CREATE TABLE job_progress (
    job_id TEXT PRIMARY KEY NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
    phase TEXT NOT NULL CHECK (phase IN ('downloading', 'deploying')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    bytes_done INTEGER,
    bytes_total INTEGER,
    deployment_progress INTEGER,
    updated_at INTEGER NOT NULL,
    CHECK (
        (phase = 'downloading'
            AND bytes_done IS NOT NULL
            AND bytes_done >= 0
            AND (bytes_total IS NULL OR bytes_total >= bytes_done)
            AND deployment_progress IS NULL)
        OR
        (phase = 'deploying'
            AND bytes_done IS NULL
            AND bytes_total IS NULL
            AND deployment_progress BETWEEN 0 AND 100)
    )
);

CREATE INDEX job_progress_updated_idx ON job_progress(updated_at);
