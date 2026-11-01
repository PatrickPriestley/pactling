-- Webhooks: an HTTP request template fired on pact and verification events, optionally
-- scoped to a consumer and/or a provider (by name; labels are stored but never match).
CREATE TABLE webhooks (
    id             INTEGER PRIMARY KEY,
    uuid           TEXT NOT NULL UNIQUE,
    description    TEXT,
    consumer_id    INTEGER REFERENCES pacticipants(id) ON DELETE CASCADE,
    provider_id    INTEGER REFERENCES pacticipants(id) ON DELETE CASCADE,
    consumer_label TEXT,
    provider_label TEXT,
    enabled        INTEGER NOT NULL DEFAULT 1,
    method         TEXT NOT NULL,
    url            TEXT NOT NULL,
    headers        TEXT,   -- JSON object, header names as given
    body           TEXT,   -- JSON encoding of the body value (object or string)
    username       TEXT,
    password       TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT
);

CREATE TABLE webhook_events (
    id         INTEGER PRIMARY KEY,
    webhook_id INTEGER NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    UNIQUE (webhook_id, name)
);

-- One row per webhook fired for an event; it outlives the webhook (webhook_id goes null).
CREATE TABLE triggered_webhooks (
    id                  INTEGER PRIMARY KEY,
    uuid                TEXT NOT NULL UNIQUE,
    webhook_id          INTEGER REFERENCES webhooks(id) ON DELETE SET NULL,
    webhook_uuid        TEXT NOT NULL,
    consumer_id         INTEGER NOT NULL REFERENCES pacticipants(id) ON DELETE CASCADE,
    provider_id         INTEGER NOT NULL REFERENCES pacticipants(id) ON DELETE CASCADE,
    pact_publication_id INTEGER REFERENCES pact_publications(id) ON DELETE CASCADE,
    verification_id     INTEGER REFERENCES verifications(id) ON DELETE CASCADE,
    trigger_type        TEXT NOT NULL,   -- resource_creation | user
    event_name          TEXT NOT NULL,
    event_context       TEXT NOT NULL,   -- JSON, as the Ruby broker's event_context
    status              TEXT NOT NULL,   -- not_run | retrying | success | failure
    next_attempt_at     TEXT,            -- RFC 3339 UTC; null once finished
    created_at          TEXT NOT NULL,
    updated_at          TEXT
);
CREATE INDEX triggered_webhooks_due ON triggered_webhooks(status, next_attempt_at);
CREATE INDEX triggered_webhooks_pact ON triggered_webhooks(pact_publication_id);
CREATE INDEX triggered_webhooks_verification ON triggered_webhooks(verification_id);

CREATE TABLE webhook_executions (
    id                   INTEGER PRIMARY KEY,
    triggered_webhook_id INTEGER NOT NULL REFERENCES triggered_webhooks(id) ON DELETE CASCADE,
    success              INTEGER NOT NULL,
    logs                 TEXT NOT NULL,
    created_at           TEXT NOT NULL
);
CREATE INDEX webhook_executions_triggered ON webhook_executions(triggered_webhook_id);
