-- Synthetic neutral assets; no telemetry values or protocol payloads are needed.
INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'),('tenant-b');
INSERT INTO identity.actors (actor_id) VALUES ('operator-a');
INSERT INTO identity.external_actor_identities (authority_id,subject,actor_id)
VALUES ('workload','operator','operator-a');
INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action)
VALUES ('tenant-a','operator-a','metadata.read'),('tenant-b','operator-a','metadata.manage');
INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit)
SELECT tenant_id,'voltage',1,'Voltage','integer','volt' FROM metadata.tenants;
INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count)
SELECT tenant_id,'equipment',1,'Equipment',1 FROM metadata.tenants;
INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal)
SELECT tenant_id,'equipment',1,'voltage',1,1 FROM metadata.tenants;
INSERT INTO metadata.relationship_type_definitions (tenant_id,relationship_type_id,version,display_name)
SELECT tenant_id,'connected',1,'Connected' FROM metadata.tenants;
INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name)
VALUES ('tenant-a','asset-a','equipment',1,'Asset A'),('tenant-a','asset-b','equipment',1,'Asset B'),
       ('tenant-a','asset-c','equipment',1,'Asset C'),('tenant-a','asset-d','equipment',1,'Asset D'),
       ('tenant-a','detached','equipment',1,'Detached'),('tenant-a','gateway-1','equipment',1,'Gateway 1'),
       ('tenant-a','gateway-2','equipment',1,'Gateway 2'),('tenant-b','asset-a','equipment',1,'Other tenant asset');
INSERT INTO metadata.communication_devices (tenant_id,device_asset_id)
VALUES ('tenant-a','gateway-1'),('tenant-a','gateway-2');
INSERT INTO metadata.device_endpoints (tenant_id,device_asset_id,endpoint_id,endpoint_kind_id,display_name,endpoint_ordinal)
VALUES ('tenant-a','gateway-1','can','can','CAN',1),('tenant-a','gateway-2','can','can','CAN',1);
INSERT INTO metadata.asset_relationships (tenant_id,relationship_id,latest_revision)
VALUES ('tenant-a','ab',1),('tenant-a','bc',1),('tenant-a','ca',1),('tenant-a','self',1);
INSERT INTO metadata.asset_relationship_revisions (tenant_id,relationship_id,revision,recorded_at_ms,relationship_type_id,
    relationship_type_version,source_asset_id,target_asset_id,effective_start_ms,effective_end_ms)
VALUES ('tenant-a','ab',1,10,'connected',1,'asset-a','asset-b',0,100),
       ('tenant-a','bc',1,10,'connected',1,'asset-b','asset-c',0,NULL),
       ('tenant-a','ca',1,10,'connected',1,'asset-c','asset-a',0,NULL),
       ('tenant-a','self',1,10,'connected',1,'asset-a','asset-a',0,NULL);
INSERT INTO metadata.signal_bindings (tenant_id,binding_id,latest_revision)
VALUES ('tenant-a','bind-1',1),('tenant-a','bind-self',1);
INSERT INTO metadata.signal_binding_revisions (tenant_id,binding_id,revision,recorded_at_ms,source_device_asset_id,
    source_endpoint_id,source_signal_id,target_asset_id,target_property_id,target_property_version,effective_start_ms,effective_end_ms)
VALUES ('tenant-a','bind-1',1,10,'gateway-1','can','bus.voltage','asset-a','voltage',1,0,100),
       ('tenant-a','bind-self',1,10,'gateway-1','can','self.voltage','gateway-1','voltage',1,0,NULL);
