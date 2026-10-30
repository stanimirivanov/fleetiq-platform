-- Explicit fixture activation: production registration never grants activation.
INSERT INTO analytics.calculator_configurations(tenant_id,configuration_id,latest_revision)
SELECT tenant_id,schedule_id,1 FROM analytics.calculator_schedules ON CONFLICT DO NOTHING;
INSERT INTO analytics.calculator_configuration_revisions(tenant_id,configuration_id,revision,schedule_id,parameters)
SELECT tenant_id,schedule_id,1,schedule_id,jsonb_build_object('calculator',calculator_id,'version',calculator_version,'scope',scope_asset_id,'target',target_asset_id,'first_start',first_start_ms,'window_ms',window_ms,'lateness_ms',lateness_ms,'minimum_samples',minimum_samples) FROM analytics.calculator_schedules ON CONFLICT DO NOTHING;
INSERT INTO analytics.calculator_activation_changes(tenant_id,configuration_id,generation,revision,actor_id)
SELECT tenant_id,configuration_id,1,1,'operator-a' FROM analytics.calculator_configurations ON CONFLICT DO NOTHING;
UPDATE analytics.calculator_configurations SET generation=1,active_revision=1;
