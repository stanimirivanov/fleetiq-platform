# ADR 0138: Minimize native backend runtime libraries

## Status

Proposed, 5 November 2026.

Supersedes the runtime-base composition in
[ADR 0110](0110-publish-qualified-immutable-backend-artifacts.md).

## Context

CI's fresh runtime scan rejected zlib1g with CVE-2026-76844 and no fixed Debian
version. The current Distroless cc-debian13 digest retains the same package.
The packaged server and three isolated executors require glibc and libgcc.
Their ELF dependency lists contain neither zlib nor OpenSSL. Rust TLS uses rustls.

## Decision

Use the digest-pinned Distroless base-nossl-debian13 image as the runtime base.
Copy libgcc_s.so.1 from a separately pinned Distroless cc-debian13 donor, together
with the libgcc-s1 and gcc-14-base package records, checksums and license/docs.
Retain the package's lintian metadata. Qualify linux/amd64 as before.

Keep the donor COPY list closed in the image policy. Require all three named
stages and immutable digests. Reject broad donor copies and missing package
evidence. Scan the resulting original runtime OCI manifest under the existing
HIGH/CRITICAL vulnerability and secret policy, including unfixed findings.

## Consequences

The runtime retains glibc, GCC unwinding and certificate data while excluding
unused TLS/compression and C++/OpenMP libraries. Package inventories and licenses
remain available to the scanner and SBOM producer.

Future native dependency changes require reviewing the runtime package closure.
Both base/donor digest updates need Rust and full image qualification. The image
remains shell-free, nonroot and read-only under the existing execution checks.

Publication and installation continue to select a fully qualified immutable
artifact through their existing review boundaries. A rollback candidate must
also satisfy the current vulnerability policy.

## Alternatives considered

Updating cc-debian13 alone retains the affected zlib package. Suppressing unfixed
findings would weaken the existing policy. Copying a library without its package
and license evidence would reduce inventory quality. Building a replacement zlib
would add a new native source/build owner for a library the application does not use.
