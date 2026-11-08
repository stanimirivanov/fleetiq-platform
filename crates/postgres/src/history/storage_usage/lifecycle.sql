-- Manifest lengths describe committed evidence. Export lengths are verified
-- by the table constraint and only count while the payload remains stored.
WITH archive AS (
    SELECT count(*) AS records, COALESCE(sum(object_bytes::numeric), 0) AS bytes
    FROM history.archive_jobs
    WHERE tenant_id = $1 AND object_sha256 IS NOT NULL
), exports AS (
    SELECT
        count(*) FILTER (WHERE status = 'ready' AND expires_at > statement_timestamp()) AS downloadable_records,
        sum(file_bytes::numeric) FILTER (WHERE status = 'ready' AND expires_at > statement_timestamp()) AS downloadable_bytes,
        count(*) FILTER (WHERE status = 'pending' AND expires_at > statement_timestamp()) AS pending_records,
        sum(file_bytes::numeric) FILTER (WHERE status = 'pending' AND expires_at > statement_timestamp()) AS pending_bytes,
        count(*) FILTER (WHERE expires_at <= statement_timestamp()) AS expired_records,
        sum(file_bytes::numeric) FILTER (WHERE expires_at <= statement_timestamp()) AS expired_bytes
    FROM history.export_jobs
    WHERE tenant_id = $1 AND payload IS NOT NULL
)
SELECT floor(extract(epoch FROM statement_timestamp()) * 1000)::bigint AS "observed!",
       archive.records::text AS "archive_records!",
       archive.bytes::text AS "archive_bytes!",
       exports.downloadable_records::text AS "downloadable_records!",
       COALESCE(exports.downloadable_bytes, 0)::text AS "downloadable_bytes!",
       exports.pending_records::text AS "pending_records!",
       COALESCE(exports.pending_bytes, 0)::text AS "pending_bytes!",
       exports.expired_records::text AS "expired_records!",
       COALESCE(exports.expired_bytes, 0)::text AS "expired_bytes!"
FROM archive CROSS JOIN exports
