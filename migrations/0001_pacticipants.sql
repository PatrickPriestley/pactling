CREATE TABLE pacticipants (
    id             INTEGER PRIMARY KEY,
    name           TEXT NOT NULL UNIQUE,
    display_name   TEXT,
    repository_url TEXT,
    main_branch    TEXT,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
