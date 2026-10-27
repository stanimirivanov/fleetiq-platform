-- Result exists only after one active tenant's <=100-fact atomic batch.
SELECT applied_count AS "applied_count!" FROM projection.project_next()
