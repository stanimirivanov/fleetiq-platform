-- Public synthetic source facts, independent of the existing voltage history fixture.
INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit)
VALUES ('tenant-a','position.latitude',1,'Latitude','integer',NULL), ('tenant-a','position.longitude',1,'Longitude','integer',NULL),
       ('tenant-b','position.latitude',1,'Latitude','integer',NULL), ('tenant-b','position.longitude',1,'Longitude','integer',NULL);
INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','located',1,'Located equipment',2);
INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal)
VALUES ('tenant-a','located',1,'position.latitude',1,1), ('tenant-a','located',1,'position.longitude',1,2);
INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name)
VALUES ('tenant-a','located-a','located',1,'Located A'), ('tenant-a','located-b','located',1,'Located B');
INSERT INTO metadata.signal_bindings (tenant_id,binding_id,latest_revision) VALUES ('tenant-a','binding-lat',1),('tenant-a','binding-lon',1);
INSERT INTO metadata.signal_binding_revisions (tenant_id,binding_id,revision,recorded_at_ms,source_device_asset_id,source_endpoint_id,source_signal_id,target_asset_id,target_property_id,target_property_version,effective_start_ms,effective_end_ms)
VALUES ('tenant-a','binding-lat',1,100,'gateway-1','can-primary','gps.latitude_e7','located-a','position.latitude',1,0,NULL),
       ('tenant-a','binding-lon',1,100,'gateway-1','can-primary','gps.longitude_e7','located-a','position.longitude',1,0,NULL);
INSERT INTO ingestion.accepted_messages (tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload)
VALUES ('tenant-a','location-message','gateway-1','vendor.protocol','1',2000,'synthetic-coordinates'::bytea);
INSERT INTO ingestion.decoded_message_results (tenant_id,message_id,protocol_package_id,protocol_package_version,observation_count) VALUES ('tenant-a','location-message','vendor.protocol','1',3);
INSERT INTO ingestion.decoded_observations (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_integer,source_unit,quality)
VALUES ('tenant-a','location-message','vendor.protocol','1',0,'can-primary','gps.latitude_e7',1000,'integer',512345678,NULL,'good'),
       ('tenant-a','location-message','vendor.protocol','1',1,'can-primary','gps.longitude_e7',1000,'integer',-123456789,NULL,'uncertain'),
       ('tenant-a','location-message','vendor.protocol','1',2,'can-primary','gps.speed',1000,'integer',99,NULL,'good');
INSERT INTO history.asset_property_changes (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis,event_at_ms,resolution_effective_at_ms,resolution_known_at_ms)
VALUES ('tenant-a','location-message','vendor.protocol','1',0,'binding-lat',1,'device_recorded',1000,1000,150),
       ('tenant-a','location-message','vendor.protocol','1',1,'binding-lon',1,'device_recorded',1000,1000,150);
