-- Exact immutable mapping definitions. Missing definitions differ from unsupported
-- representation. No property-name or vendor convention supplies units/scaling.
SELECT COUNT(*) AS "found!",
       COALESCE(bool_and(CASE WHEN $6::boolean THEN value_kind='integer' AND canonical_unit IS NULL
                            ELSE value_kind IN ('integer','decimal') AND canonical_unit IS NOT DISTINCT FROM 'deg' END), false) AS "supported!"
FROM metadata.property_definitions
WHERE tenant_id=$1 AND ((property_id=$2 AND version=$3) OR (property_id=$4 AND version=$5))
