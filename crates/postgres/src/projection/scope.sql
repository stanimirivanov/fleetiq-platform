-- Only exact installed property versions exist in this snapshot namespace.
SELECT EXISTS (
    SELECT 1 FROM metadata.assets a JOIN metadata.asset_type_properties p
      ON p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version
    WHERE a.tenant_id=$1 AND a.asset_id=$2 AND p.property_id=$3 AND p.property_version=$4
) AS "exists!"
