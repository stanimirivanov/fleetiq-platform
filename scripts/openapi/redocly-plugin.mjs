import { dirname } from 'node:path';
import { formatIssue, inspectLayout } from './layout.mjs';
import { assertReviewedIgnorePolicy } from './ignore-policy.mjs';
import { opaquePayloadTypes } from './opaque-types.mjs';

// Redocly owns parsing/resolution. The bounded resource walker distinguishes
// actual OpenAPI structure from arbitrary payloads, annotations, and extensions.
export default function fleetIqOpenapiPlugin() {
  return {
    id: 'fleetiq',
    typeExtension: { oas3: opaquePayloadTypes },
    rules: {
      oas3: {
        layout: () => ({
          Root: {
            leave(document, context) {
              assertReviewedIgnorePolicy(context.config.ignore, context.config.configPath);
              const result = inspectLayout({
                rootDirectory: dirname(context.location.source.absoluteRef),
                document,
                resolveDocument: (reference, target) => {
                  const resolved = context.resolve(reference);
                  // Whole-file aliases collapse before visiting intermediate refs;
                  // require every registered resource to retain its owning source.
                  if (resolved.location?.source.absoluteRef !== target) throw new Error('whole-file alias');
                  return resolved.node;
                },
              });
              for (const issue of result.issues) context.report({ message: formatIssue(issue), location: context.location });
            },
          },
        }),
      },
    },
  };
}
