-- One row per published verification result. `number` is the broker-wide sequence the
-- Ruby broker exposes in URLs and names ("Verification result 7 for ...").
CREATE TABLE verifications (
    id                  INTEGER PRIMARY KEY,
    number              INTEGER NOT NULL UNIQUE,
    pact_version_id     INTEGER NOT NULL REFERENCES pact_versions(id) ON DELETE CASCADE,
    provider_version_id INTEGER NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    consumer_version_id INTEGER REFERENCES versions(id) ON DELETE SET NULL,
    success             INTEGER NOT NULL,
    test_results        TEXT,
    verified_by         TEXT,
    build_url           TEXT,
    created_at          TEXT NOT NULL
);
CREATE INDEX verifications_pact_version ON verifications(pact_version_id);
CREATE INDEX verifications_provider_version ON verifications(provider_version_id);
