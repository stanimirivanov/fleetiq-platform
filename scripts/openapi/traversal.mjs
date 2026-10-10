const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const operations = ['get', 'put', 'post', 'delete', 'options', 'head', 'patch', 'trace'];
const referenceKinds = new Set(['PathItem', 'Schema', 'Parameter', 'Header', 'Response', 'RequestBody', 'Example', 'Link', 'Callback', 'SecurityScheme']);
const schemaMaps = ['properties', 'patternProperties', '$defs', 'definitions', 'dependentSchemas', 'dependencies'];
const schemaChildren = ['items', 'additionalItems', 'contains', 'additionalProperties', 'unevaluatedProperties', 'propertyNames', 'if', 'then', 'else', 'not', 'unevaluatedItems', 'contentSchema'];
const schemaArrays = ['allOf', 'anyOf', 'oneOf', 'prefixItems'];

export function resourceKind(name) {
  if (name.startsWith('paths/')) return 'PathItem';
  return new Map([
    ['schemas', 'Schema'], ['parameters', 'Parameter'], ['headers', 'Header'],
    ['responses', 'Response'], ['security-schemes', 'SecurityScheme'],
  ]).get(name.split('/')[1]);
}

/** Visit only OpenAPI and JSON Schema structural locations in registered resources.
 * Example payloads, annotation values, and extensions are arbitrary JSON data.
 * Redocly still owns parsing, type validity, and fragment resolution.
 */
export function walkResource(document, kind, callbacks) {
  function visit(node, currentKind, parts = [], ancestors = new Set()) {
    if (!object(node)) return;
    if (ancestors.has(node)) { callbacks.cycle(parts); return; }
    const nextAncestors = new Set(ancestors).add(node);
    if (referenceKinds.has(currentKind) && Object.hasOwn(node, '$ref')) callbacks.reference([...parts, '$ref'], node.$ref);
    const child = (key, childKind) => visit(node[key], childKind, [...parts, key], nextAncestors);
    const map = (key, childKind) => {
      if (!object(node[key])) return;
      for (const [name, value] of Object.entries(node[key])) visit(value, childKind, [...parts, key, name], nextAncestors);
    };
    const schema = () => {
      if (!Object.hasOwn(node, 'schema')) return;
      callbacks.schema([...parts, 'schema'], node.schema);
      child('schema', 'Schema');
    };
    const parameters = () => {
      if (!Array.isArray(node.parameters)) return;
      for (const [index, value] of node.parameters.entries()) {
        const location = [...parts, 'parameters', index];
        callbacks.parameter(location, value);
        visit(value, 'Parameter', location, nextAncestors);
      }
    };
    switch (currentKind) {
      case 'PathItem':
        parameters();
        for (const operation of operations) child(operation, 'Operation');
        break;
      case 'Operation':
        parameters();
        child('requestBody', 'RequestBody');
        map('responses', 'Response');
        map('callbacks', 'Callback');
        break;
      case 'Schema':
        for (const key of schemaMaps) map(key, 'Schema');
        for (const key of schemaChildren) child(key, 'Schema');
        for (const key of schemaArrays) {
          if (Array.isArray(node[key])) for (const [index, value] of node[key].entries()) visit(value, 'Schema', [...parts, key, index], nextAncestors);
        }
        break;
      case 'Parameter':
      case 'Header':
        schema();
        map('content', 'MediaType');
        map('examples', 'Example');
        break;
      case 'Response':
        map('headers', 'Header');
        map('content', 'MediaType');
        map('links', 'Link');
        break;
      case 'RequestBody':
        map('content', 'MediaType');
        break;
      case 'MediaType':
        schema();
        map('examples', 'Example');
        map('encoding', 'Encoding');
        break;
      case 'Encoding':
        map('headers', 'Header');
        break;
      case 'Callback':
        for (const [name, value] of Object.entries(node)) {
          if (name !== '$ref' && !name.startsWith('x-')) visit(value, 'PathItem', [...parts, name], nextAncestors);
        }
        break;
      // Example.value, Link.parameters/requestBody, SecurityScheme extensions,
      // and Schema const/default/enum/examples are intentionally opaque.
      default:
        break;
    }
  }
  visit(document, kind);
}
