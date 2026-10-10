// Public type-extension metadata controls traversal only. Literal annotations
// and extensions stay JSON data; normative fields retain their validators.
export function opaquePayloadTypes(types) {
  const result = { ...types };
  for (const [name, type] of Object.entries(types)) {
    if (type.extensionsPrefix !== 'x-') continue;
    const original = type.additionalProperties;
    result[name] = {
      ...type,
      extensionsPrefix: undefined,
      additionalProperties(node, property) {
        if (property.startsWith('x-')) return { resolvable: false };
        return typeof original === 'function' ? original(node, property) : original;
      },
    };
  }
  const schema = result.Schema;
  const properties = { ...schema.properties };
  for (const key of ['const', 'default', 'enum', 'examples']) {
    if (Object.hasOwn(properties, key)) properties[key] = { ...properties[key], resolvable: false };
  }
  result.Schema = { ...schema, properties };
  return result;
}