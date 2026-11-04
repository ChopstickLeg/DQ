ALTER TABLE jobs ADD COLUMN result BLOB;
ALTER TABLE jobs ADD COLUMN error TEXT;
ALTER TABLE jobs ADD COLUMN completed_attempt_id TEXT;

CREATE INDEX jobs_pending_by_route ON jobs (service_name, function_name, id)
    WHERE status = 1;
