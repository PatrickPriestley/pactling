CREATE TABLE environments (
    id           INTEGER PRIMARY KEY,
    uuid         TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL UNIQUE,
    display_name TEXT,
    production   INTEGER NOT NULL DEFAULT 0,
    contacts     TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT
);

-- A record of a pacticipant version being deployed to an environment. Recording a newer
-- version for the same pacticipant, environment and application instance marks the
-- earlier record undeployed.
CREATE TABLE deployed_versions (
    id                   INTEGER PRIMARY KEY,
    uuid                 TEXT NOT NULL UNIQUE,
    version_id           INTEGER NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    environment_id       INTEGER NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    application_instance TEXT,
    currently_deployed   INTEGER NOT NULL DEFAULT 1,
    undeployed_at        TEXT,
    created_at           TEXT NOT NULL
);
CREATE INDEX deployed_versions_env ON deployed_versions(environment_id, currently_deployed);
CREATE INDEX deployed_versions_version ON deployed_versions(version_id);

CREATE TABLE released_versions (
    id                  INTEGER PRIMARY KEY,
    uuid                TEXT NOT NULL UNIQUE,
    version_id          INTEGER NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    environment_id      INTEGER NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    currently_supported INTEGER NOT NULL DEFAULT 1,
    support_ended_at    TEXT,
    created_at          TEXT NOT NULL,
    UNIQUE (version_id, environment_id)
);
CREATE INDEX released_versions_env ON released_versions(environment_id, currently_supported);
