-- Use the same database text collation, numeric revision and basis ordering as hot history.
WITH entries AS (
 SELECT * FROM jsonb_to_recordset($1::jsonb) AS x(event BIGINT,message TEXT,package TEXT,version TEXT,
 output BIGINT,binding TEXT,revision NUMERIC,basis TEXT,index BIGINT)
), position AS (
 SELECT * FROM jsonb_to_record($2::jsonb) AS x(event BIGINT,message TEXT,package TEXT,version TEXT,
 output BIGINT,binding TEXT,revision NUMERIC,basis TEXT,index BIGINT)
)
SELECT e.index AS "index!" FROM entries e CROSS JOIN position p
WHERE p.event IS NULL OR (e.event,e.message,e.package,e.version,e.output,e.binding,e.revision,e.basis)
 > (p.event,p.message,p.package,p.version,p.output,p.binding,p.revision,p.basis)
ORDER BY e.event,e.message,e.package,e.version,e.output,e.binding,e.revision,e.basis
LIMIT 116
