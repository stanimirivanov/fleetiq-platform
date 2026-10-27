-- Function validates transaction isolation and active lifecycle before mutation.
SELECT TRUE AS "done!" FROM projection.rebuild_current($1)
