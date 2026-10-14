# Browser operator identity

## TL;DR

Serve the UI and `/api` on one HTTPS public origin. Configure a trusted OIDC
issuer and confidential client, register the exact callback, provision each
verified subject against the configured identity authority, and grant tenant
`metadata.read` access. The browser receives a protected session cookie; the
configured workload bearer must never be placed in UX configuration.

## Configuration and trust

| Variable | Meaning |
|:--|:--|
| `FLEETIQ_BROWSER_ORIGIN` | HTTPS origin only, such as `https://fleet.example.com`; no credentials, path, query, or fragment |
| `FLEETIQ_OIDC_ISSUER` | Exact HTTPS issuer for discovery and verification; provider issuer paths are allowed |
| `FLEETIQ_OIDC_CLIENT_ID` | Confidential OIDC client registered with the provider |
| `FLEETIQ_OIDC_CLIENT_SECRET` | Backend-only secret injected through the deployment environment |
| `FLEETIQ_OIDC_IDENTITY_AUTHORITY` | Stable FleetIQ authority identifier for this exact issuer |

Configure all five values together. Partial configuration or simultaneous
workload bearer and browser modes fails startup. Discovery must succeed before
the listener starts. Register the exact callback
`https://fleet.example.com/api/v1/auth/callback`, authorization-code flow,
PKCE S256, and `openid` scope with the provider. The provider must publish
signing public keys and support `client_secret_basic` or `client_secret_post`.
Signing-key rotation is discovered on callbacks; client-secret rotation
requires updated configuration and restart.

Terminate HTTPS at a trusted edge, serve the built UI, and forward `/api` to the
private backend listener. Preserve `Origin` and `Set-Cookie`; do not cache API
responses or expose the plaintext listener publicly. The server neither trusts
forwarding headers for redirects nor enables credentialed CORS. Local browser
verification also requires a trusted HTTPS origin and provider; use a local
TLS edge instead of weakening issuer and cookie validation.

The default [Compose setup](../development/README.md) exposes only health and
discovery until identity is explicitly configured. Its PostgreSQL host port
55432 is isolated and can be changed independently of other projects.

## Provisioning

Use existing authorized identity use cases to register an active actor with
the exact `(authority, subject)` mapping. Authority is the configured FleetIQ
identifier; subject is the verified stable OIDC `sub` claim. Email, groups, and
provider roles do not grant access or automatically create actors. Create an
active tenant and its `metadata.read` grant through trusted administration.
A valid provider identity without a registered active actor receives generic
401; an actor without access to the selected tenant receives 403.

Use a different authority for a different issuer and deliberately provision its
operator mappings. Changing the configured issuer, client ID, or authority
invalidates existing attempts and sessions through their stored trust binding.
Client-secret rotation preserves that binding. Browser login creates neither
actors nor grants. Authorized operators can use the protected
[actor administration endpoints](actor-administration.md) for provisioning and
lifecycle changes; browser commands require the same exact Origin and
X-FleetIQ-CSRF marker as logout, plus current platform identity.manage authority.
Additional identity mappings and grant HTTP administration remain separate.

## Browser workflow

1. Navigate to `GET /api/v1/auth/login`. The server stores a five-minute
   browser-bound attempt, sets `__Host-fleetiq-login`, and redirects with 303.
2. The provider returns code and state to `GET /api/v1/auth/callback`. The
   backend consumes the bound attempt once, exchanges and verifies the code,
   resolves the operator, rotates the session, and redirects to `/`.
   Caller-supplied redirect destinations are not accepted.
3. Read `GET /api/v1/auth/session` for `actor_id` and `expires_at_ms`.
   Same-origin fetch sends the session cookie. Protected metadata reads return
   structured 401 instead of redirecting automatically when sign-in is needed.
4. Send `POST /api/v1/auth/logout` with the configured exact `Origin` and
   `X-FleetIQ-CSRF: 1`. The server deletes the session and this browser's
   still-pending login attempts, then clears both cookies. Logout is idempotent
   for a browser without an active session.

The session cookie `__Host-fleetiq-session` is `Secure`, `HttpOnly`,
`SameSite=Lax`, `Path=/`, without `Domain`. Provider tokens and raw browser
credentials never appear in backend JSON, PostgreSQL session rows, or routine
diagnostics. A short-lived pending attempt retains its PKCE verifier and nonce
under database privileges. The verifier remains backend-only; state and nonce
are provider authorization parameters carried in the redirect.

## Expiry, revocation, and rollback

Absolute session expiry is the earlier of eight hours and ID-token expiry.
There is no refresh token or sliding renewal. Expired sessions are rejected
before periodic cleanup. In browser mode, cleanup runs every 60 seconds; a
failure produces a generic diagnostic and is retried on the next tick. Cleanup
failure never extends credential validity. Restrict database access to the
application and administrators.

Actor suspension, mapping unlink/reassignment, tenant suspension, and grant
revocation affect subsequent operations. Sessions retain the actor established at
login, so reassignment cannot transfer access. Already accepted in-flight operations
may complete. A callback that consumed its one-use attempt before sign-out may
also complete and establish a fresh session; this slice does not provide
cancellation generations for concurrent sign-in. Provider-only account
revocation is bounded by session expiry; immediate back-channel provider
logout is not implemented.

Disable all five browser settings and restart to roll back. Browser routes and
cookie authentication disappear; additive tables may remain. A trusted workload
deployment may separately restore configured bearer mode. Never copy that
credential into the UI as a fallback.

## Qualification and references

Automated tests cover controlled signed provider tokens, real PostgreSQL login
consumption and sessions, and HTTP authentication and authorization journeys.
These tests do not qualify a deployed customer provider, TLS edge, browser
deployment, or operator provisioning. Verify those boundaries before production
use. Configure bounded request sizes and sign-in rate limits at the edge:
anonymous login persists short-lived attempts, and provider callbacks perform
network requests. Deployment qualification remains separate from the controlled test fixtures.

[ADR 0095](../adr/0095-authenticate-browser-operators-with-oidc-sessions.md)
records the boundary. The maintained
[OIDC library](https://docs.rs/openidconnect/4.0.1/openidconnect/) implements
protocol verification. The
[dependency review](../development/plans/browser-identity-dependency-review.md)
documents its public-verification-only RSA advisory assessment and approved
security-policy exception.
