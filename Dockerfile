# syntax=docker/dockerfile:1.28.0@sha256:bb22d9815c728170f72750f4e5b0d672e06176142e1d602c7e66c050100b7e5b

# Compile with the repository-pinned Rust release. SQLx uses the committed
# offline query descriptions so an image build never needs database access.
FROM rust:1.99.1-slim-bookworm@sha256:86234e857a413004348b471d11a4ebe1aa92d2eae2b6b143cac68fd1ac3c48be AS builder

ARG BUILDKIT_SBOM_SCAN_STAGE=true

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY examples/custom-protocol ./examples/custom-protocol
COPY packages/teltonika ./packages/teltonika
COPY migrations ./migrations
COPY .sqlx ./.sqlx

RUN --mount=type=cache,id=fleetiq-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=fleetiq-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=fleetiq-target,target=/workspace/target,sharing=locked \
    SQLX_OFFLINE=true cargo build --locked --release -p fleetiq-server -p fleetiq-protocol-wasm --bins && \
    cp target/release/fleetiq-server /tmp/fleetiq-server && \
    cp target/release/fleetiq-protocol-admission /tmp/fleetiq-protocol-admission && \
    cp target/release/fleetiq-protocol-decode /tmp/fleetiq-protocol-decode && \
    cp target/release/fleetiq-protocol-live /tmp/fleetiq-protocol-live

# Generate the inventory before entering the shell-free runtime stage.
RUN /tmp/fleetiq-server migration-inventory > /tmp/migrations.json

# The Rust executable needs glibc and libgcc, not a package manager, shell or curl.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1 AS runtime-libraries
FROM gcr.io/distroless/base-nossl-debian13:nonroot@sha256:e6b0f82595aa1edde72c8054de7a9dca30f0f8f54820c37a167c0ddfc21c5c67 AS runtime

# Rust uses rustls and needs glibc/libgcc, so keep unused OpenSSL/compression out.
# Carry the complete libgcc package evidence and license alongside its library.
COPY --from=runtime-libraries /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 /usr/lib/x86_64-linux-gnu/libgcc_s.so.1
COPY --from=runtime-libraries /var/lib/dpkg/status.d/libgcc-s1 /var/lib/dpkg/status.d/libgcc-s1
COPY --from=runtime-libraries /var/lib/dpkg/status.d/libgcc-s1.md5sums /var/lib/dpkg/status.d/libgcc-s1.md5sums
COPY --from=runtime-libraries /var/lib/dpkg/status.d/gcc-14-base /var/lib/dpkg/status.d/gcc-14-base
COPY --from=runtime-libraries /var/lib/dpkg/status.d/gcc-14-base.md5sums /var/lib/dpkg/status.d/gcc-14-base.md5sums
COPY --from=runtime-libraries /usr/share/doc/gcc-14-base/ /usr/share/doc/gcc-14-base/
COPY --from=runtime-libraries /usr/share/doc/libgcc-s1/ /usr/share/doc/libgcc-s1/
COPY --from=runtime-libraries /usr/share/lintian/overrides/libgcc-s1 /usr/share/lintian/overrides/libgcc-s1

COPY --from=builder /tmp/fleetiq-server /usr/local/bin/fleetiq-server
COPY --from=builder /tmp/fleetiq-protocol-admission /usr/local/bin/fleetiq-protocol-admission
COPY --from=builder /tmp/fleetiq-protocol-decode /usr/local/bin/fleetiq-protocol-decode
COPY --from=builder /tmp/fleetiq-protocol-live /usr/local/bin/fleetiq-protocol-live
COPY --from=builder /tmp/migrations.json /usr/share/fleetiq/migrations.json

ENV FLEETIQ_BIND_ADDRESS=0.0.0.0:8080

ARG SOURCE_REVISION=development
LABEL org.opencontainers.image.source="https://github.com/stanimirivanov/fleetiq-platform" \
      org.opencontainers.image.revision="${SOURCE_REVISION}"

EXPOSE 8080

USER 10001:10001

HEALTHCHECK --interval=5s --timeout=3s --start-period=10s --retries=10 \
    CMD ["/usr/local/bin/fleetiq-server", "healthcheck"]

ENTRYPOINT ["/usr/local/bin/fleetiq-server"]

# Production never acquires DDL authority implicitly. Development opts in explicitly.
CMD ["serve", "--check-schema"]
