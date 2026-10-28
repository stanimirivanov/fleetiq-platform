-- Fetch one sentinel row beyond the bounded page. C collation gives a stable
-- byte-ordered cursor, independent of the database's locale and client collation.
SELECT a.asset_id, a.display_name, a.type_id, a.type_version,
       (EXISTS (SELECT 1 FROM metadata.asset_type_properties p WHERE p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version AND p.property_id=$2 AND p.property_version=$3)
        AND EXISTS (SELECT 1 FROM metadata.asset_type_properties p WHERE p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version AND p.property_id=$4 AND p.property_version=$5)) AS "mapped!"
FROM metadata.assets a
WHERE a.tenant_id=$1 AND ($6::text IS NULL OR a.asset_id COLLATE "C" > $6 COLLATE "C")
ORDER BY a.asset_id COLLATE "C" LIMIT $7
