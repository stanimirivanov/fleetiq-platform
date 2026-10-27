SELECT message.message_id, message.protocol_package_id, message.protocol_package_version,
       observation.output_index, change.binding_id, change.binding_revision,
       change.event_time_basis, change.event_at_ms, change.resolution_effective_at_ms,
       change.resolution_known_at_ms, binding.target_asset_id, binding.target_property_id,
       binding.target_property_version, message.device_asset_id, message.received_at_ms,
       observation.endpoint_id, observation.signal_id, observation.device_recorded_at_ms,
       observation.value_kind, observation.value_boolean, observation.value_integer,
       observation.value_decimal, observation.value_text, observation.source_unit, observation.quality
FROM projection.current_properties incumbent
JOIN history.asset_property_changes AS change
  ON change.tenant_id=incumbent.tenant_id AND change.message_id=incumbent.message_id
 AND change.protocol_package_id=incumbent.protocol_package_id AND change.protocol_package_version=incumbent.protocol_package_version
 AND change.output_index=incumbent.output_index AND change.binding_id=incumbent.binding_id
 AND change.binding_revision=incumbent.binding_revision AND change.event_time_basis=incumbent.event_time_basis
JOIN metadata.signal_binding_revisions AS binding
  ON binding.tenant_id=change.tenant_id AND binding.binding_id=change.binding_id AND binding.revision=change.binding_revision
JOIN ingestion.decoded_observations AS observation
  ON observation.tenant_id=change.tenant_id AND observation.message_id=change.message_id
 AND observation.protocol_package_id=change.protocol_package_id AND observation.protocol_package_version=change.protocol_package_version
 AND observation.output_index=change.output_index
JOIN ingestion.accepted_messages AS message
  ON message.tenant_id=observation.tenant_id AND message.message_id=observation.message_id
 AND message.protocol_package_id=observation.protocol_package_id AND message.protocol_package_version=observation.protocol_package_version
WHERE incumbent.tenant_id=$1 AND incumbent.asset_id=$2 AND incumbent.property_id=$3 AND incumbent.property_version=$4
