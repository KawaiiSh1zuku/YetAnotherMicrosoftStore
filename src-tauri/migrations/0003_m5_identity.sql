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
