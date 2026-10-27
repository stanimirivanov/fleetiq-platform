INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'), ('tenant-b');
INSERT INTO identity.actors (actor_id) VALUES ('operator-a');
INSERT INTO identity.external_actor_identities (authority_id, subject, actor_id)
VALUES ('workload','operator','operator-a');
INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action)
VALUES ('tenant-a','operator-a','telemetry.read'), ('tenant-b','operator-a','metadata.read');
INSERT INTO metadata.property_definitions
    (tenant_id,property_id,version,display_name,value_kind,canonical_unit)
VALUES ('tenant-a','electrical.voltage',1,'Voltage','integer','volt'),
       ('tenant-a','electrical.voltage',2,'Voltage v2','integer','volt'),
       ('tenant-a','exact.decimal',1,'Exact decimal','decimal',NULL),
       ('tenant-a','switch.state',1,'Switch','boolean',NULL),
       ('tenant-a','status.text',1,'Text','text',NULL);
INSERT INTO metadata.asset_type_definitions
    (tenant_id,type_id,version,display_name,property_count)
VALUES ('tenant-a','equipment',1,'Equipment',4);
INSERT INTO metadata.asset_type_properties
    (tenant_id,type_id,type_version,property_id,property_version,property_ordinal)
VALUES ('tenant-a','equipment',1,'electrical.voltage',1,1),
       ('tenant-a','equipment',1,'exact.decimal',1,2),
       ('tenant-a','equipment',1,'switch.state',1,3),
       ('tenant-a','equipment',1,'status.text',1,4);
INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name)
VALUES ('tenant-a','gateway-1','equipment',1,'Gateway'),
       ('tenant-a','asset-a','equipment',1,'Asset A'),
       ('tenant-a','asset-b','equipment',1,'Asset B');
INSERT INTO metadata.communication_devices (tenant_id,device_asset_id) VALUES ('tenant-a','gateway-1');
INSERT INTO metadata.device_endpoints
    (tenant_id,device_asset_id,endpoint_id,endpoint_kind_id,display_name,endpoint_ordinal)
VALUES ('tenant-a','gateway-1','can-primary','can','CAN',1);
INSERT INTO metadata.signal_bindings (tenant_id,binding_id,latest_revision)
VALUES ('tenant-a','binding-1',1), ('tenant-a','binding-decimal',1);
INSERT INTO metadata.signal_binding_revisions
    (tenant_id,binding_id,revision,recorded_at_ms,source_device_asset_id,source_endpoint_id,source_signal_id,
     target_asset_id,target_property_id,target_property_version,effective_start_ms,effective_end_ms)
VALUES ('tenant-a','binding-1',1,100,'gateway-1','can-primary','bus.voltage','asset-a','electrical.voltage',1,0,NULL),
       ('tenant-a','binding-decimal',1,100,'gateway-1','can-primary','exact','asset-a','exact.decimal',1,0,NULL);
INSERT INTO ingestion.accepted_messages
    (tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload)
VALUES ('tenant-a','message-1','gateway-1','vendor.protocol','1',2000,'private-wire-sentinel'::bytea);
INSERT INTO ingestion.decoded_message_results
    (tenant_id,message_id,protocol_package_id,protocol_package_version,observation_count)
VALUES ('tenant-a','message-1','vendor.protocol','1',107);
INSERT INTO ingestion.decoded_observations
    (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,
     device_recorded_at_ms,value_kind,value_integer,source_unit,quality)
SELECT 'tenant-a','message-1','vendor.protocol','1',n,'can-primary','bus.voltage',CASE WHEN n=1 THEN 500 ELSE 1000 END,'integer',9223372036854775807,'volt','good' FROM generate_series(0,106) AS n;
INSERT INTO history.asset_property_changes
    (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,
     event_time_basis,event_at_ms,resolution_effective_at_ms,resolution_known_at_ms)
VALUES ('tenant-a','message-1','vendor.protocol','1',0,'binding-1',1,'device_recorded',1000,1000,150),
       ('tenant-a','message-1','vendor.protocol','1',0,'binding-1',1,'received_at',2000,1000,150);
