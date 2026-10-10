import { resolve, dirname } from 'node:path';

/** Only the two reviewed OAuth navigation exceptions may suppress lint output. */
export function assertReviewedIgnorePolicy(ignore, configPath) {
  const parent = dirname(configPath);
  const reviewed = new Set([
    resolve(parent, 'openapi/paths/identity/login.yaml'),
    resolve(parent, 'openapi/paths/identity/callback.yaml'),
  ]);
  for (const [file, rules] of Object.entries(ignore ?? {})) {
    for (const [rule, pointers] of Object.entries(rules)) {
      const values = Array.from(pointers);
      if (!reviewed.has(resolve(file)) || rule !== 'operation-2xx-response' || values.length !== 1 || values[0] !== '#/get/responses') {
        // A reported finding could itself be ignored. Throw before inspecting
        // layout, so an ignore entry cannot suppress this policy failure.
        throw new Error('OpenAPI ignore policy permits only the two exact reviewed OAuth operation-2xx-response exceptions; structural findings cannot be suppressed.');
      }
    }
  }
}
