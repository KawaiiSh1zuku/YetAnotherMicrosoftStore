ALTER TABLE package_versions ADD COLUMN publisher TEXT;
ALTER TABLE package_versions ADD COLUMN resource_id TEXT;
ALTER TABLE package_versions
    ADD COLUMN package_kind TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE package_versions ADD COLUMN minimum_os_version TEXT;
ALTER TABLE package_versions ADD COLUMN is_neutral INTEGER;
ALTER TABLE package_versions ADD COLUMN content_id TEXT;

CREATE INDEX package_versions_applicability_idx
    ON package_versions(product_id, package_kind, architecture, language, market);
CREATE INDEX package_versions_content_idx
    ON package_versions(content_id);
