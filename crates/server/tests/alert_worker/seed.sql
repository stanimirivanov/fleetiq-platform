-- Test-owned synthetic rule and scheduled evidence; never customer input.
INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','alerts.manage');
      INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);
      SELECT analytics.create_alert_rule_revision('tenant-a','high',1,'operator-a','{"kind":"numeric-threshold-v1","calculator":"mean","version":"1","scope":"asset-a","target":"result","output_property":"mean.voltage","output_version":1,"output_index":0,"comparison":"gt","threshold":"50","threshold_unit":"volt","hold_ms":0,"resolution":"resolve_on_false","resolve_after_ms":0}');
      SELECT analytics.change_alert_rule_activation('tenant-a','high',0,1,'operator-a');
      INSERT INTO analytics.calculator_schedules(tenant_id,schedule_id,calculator_id,calculator_version,configuration_id,scope_asset_id,target_asset_id,first_start_ms,window_ms,lateness_ms,minimum_samples,next_start_ms)
        VALUES('tenant-a','s','mean','1','c','asset-a','result',0,1000,0,1,0);
      WITH evidence AS (SELECT jsonb_build_object('scope','asset-a','calculator','mean','version','1','configuration','c',
          'start',recorded_at_ms,'end',recorded_at_ms+1000,'finalize',recorded_at_ms+1000,'known',recorded_at_ms+1000,'inputs','[]'::jsonb) AS k
        FROM analytics.alert_rule_activation_changes WHERE tenant_id='tenant-a' AND rule_id='high' AND generation=1)
      INSERT INTO analytics.evaluations(tenant_id,evaluation_id,replay_key,key_digest,evaluated_at_ms,outcome,outputs,minimum_samples)
        SELECT 'tenant-a','e',k,sha256(convert_to(k::text,'UTF8')),(k->>'end')::bigint,'complete',
          '[{"asset":"result","property":"mean.voltage","version":1,"kind":"decimal","value":"51","unit":"volt"}]'::jsonb,1 FROM evidence;
      INSERT INTO analytics.calculator_work(tenant_id,evaluation_id,schedule_id,start_ms,request,generation,status)
        VALUES('tenant-a','e','s',0,'{"target":"result"}',1,'done');
