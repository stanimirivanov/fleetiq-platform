-- Counts cover the whole tenant, not the current page or cursor suffix. They
-- describe inventory/configuration/materialization, never online/healthy assets.
SELECT (SELECT COUNT(*) FROM metadata.assets WHERE tenant_id=$1) AS "assets!",
       (SELECT COUNT(*) FROM metadata.communication_devices WHERE tenant_id=$1) AS "devices!",
       (SELECT COUNT(*) FROM metadata.assets a WHERE a.tenant_id=$1
         AND EXISTS (SELECT 1 FROM metadata.asset_type_properties p WHERE p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version AND p.property_id=$2 AND p.property_version=$3)
         AND EXISTS (SELECT 1 FROM metadata.asset_type_properties p WHERE p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version AND p.property_id=$4 AND p.property_version=$5)) AS "mapped!",
       (SELECT COUNT(DISTINCT c.asset_id) FROM projection.current_properties c JOIN metadata.assets a ON a.tenant_id=c.tenant_id AND a.asset_id=c.asset_id WHERE c.tenant_id=$1) AS "projected!"
