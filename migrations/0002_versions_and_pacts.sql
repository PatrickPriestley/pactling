-- Versions are ordered by creation; `id` is the Ruby broker's `order`.
CREATE TABLE versions (
    id             INTEGER PRIMARY KEY,
    pacticipant_id INTEGER NOT NULL REFERENCES pacticipants(id) ON DELETE CASCADE,
    number         TEXT NOT NULL,
    build_url      TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT,
    UNIQUE (pacticipant_id, number)
);

CREATE TABLE branches (
    id             INTEGER PRIMARY KEY,
    pacticipant_id INTEGER NOT NULL REFERENCES pacticipants(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT,
    UNIQUE (pacticipant_id, name)
);

CREATE TABLE branch_versions (
    id         INTEGER PRIMARY KEY,
    branch_id  INTEGER NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    version_id INTEGER NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    UNIQUE (branch_id, version_id)
);
CREATE INDEX branch_versions_version ON branch_versions(version_id);

CREATE TABLE tags (
    id         INTEGER PRIMARY KEY,
    version_id INTEGER NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (version_id, name)
);

-- One row per distinct pact content for a consumer/provider pair.
CREATE TABLE pact_versions (
    id          INTEGER PRIMARY KEY,
    consumer_id INTEGER NOT NULL REFERENCES pacticipants(id) ON DELETE CASCADE,
    provider_id INTEGER NOT NULL REFERENCES pacticipants(id) ON DELETE CASCADE,
    sha         TEXT NOT NULL,
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (consumer_id, provider_id, sha)
);

-- Which content a consumer version published for a provider.
CREATE TABLE pact_publications (
    id                  INTEGER PRIMARY KEY,
    consumer_version_id INTEGER NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    provider_id         INTEGER NOT NULL REFERENCES pacticipants(id) ON DELETE CASCADE,
    pact_version_id     INTEGER NOT NULL REFERENCES pact_versions(id) ON DELETE CASCADE,
    created_at          TEXT NOT NULL,
    updated_at          TEXT,
    UNIQUE (consumer_version_id, provider_id)
);
CREATE INDEX pact_publications_provider ON pact_publications(provider_id);
