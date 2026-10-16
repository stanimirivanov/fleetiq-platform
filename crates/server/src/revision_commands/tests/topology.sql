INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'),('tenant-b');
INSERT INTO identity.actors (actor_id) VALUES ('operator-a'),('operator-b');
INSERT INTO identity.external_actor_identities (authority_id,subject,actor_id) VALUES ('workload','operator','operator-a');
INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action)
VALUES ('tenant-a','operator-a','metadata.read'),('tenant-a','operator-a','metadata.manage');
INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind)
SELECT tenant_id,'voltage',1,'Voltage','decimal' FROM metadata.tenants;
INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count)
SELECT tenant_id,'equipment',1,'Equipment',1 FROM metadata.tenants;
INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal)
SELECT tenant_id,'equipment',1,'voltage',1,1 FROM metadata.tenants;
INSERT INTO metadata.relationship_type_definitions (tenant_id,relationship_type_id,version,display_name)
SELECT tenant_id,'physical.contains',1,'Contains' FROM metadata.tenants;
INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name)
SELECT tenant_id,asset_id,'equipment',1,asset_id FROM metadata.tenants CROSS JOIN (VALUES ('asset-a'),('asset-b'),('asset-c')) AS ids(asset_id);
INSERT INTO metadata.communication_devices (tenant_id,device_asset_id) SELECT tenant_id,'asset-a' FROM metadata.tenants;
INSERT INTO metadata.device_endpoints (tenant_id,device_asset_id,endpoint_id,endpoint_kind_id,display_name,endpoint_ordinal)
SELECT tenant_id,'asset-a','can','bus.can','CAN',1 FROM metadata.tenants;
