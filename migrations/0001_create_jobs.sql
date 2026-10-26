CREATE TABLE jobs (
    id STRING PRIMARY KEY,
    function_name TEXT NOT NULL,
    service_name TEXT NOT NULL,
    submission_key TEXT NOT NULL,
    arguments BLOB NOT NULL,
    caller TEXT NOT NULL,
    status INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TEXT,
    UNIQUE (function_name, submission_key)
);