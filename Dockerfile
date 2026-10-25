# syntax=docker/dockerfile:1.28.0@sha256:bb22d9815c728170f72750f4e5b0d672e06176142e1d602c7e66c050100b7e5b

# Compile with the repository-pinned Rust release. SQLx uses the committed
# offline query descriptions so an image build never needs database access.
FROM rust:1.98.1-slim-bookworm@sha256:ff521445a372125ed4f76e1453a1f8098f2d05332d1601d30db1c1f62757e730 AS builder

ARG BUILDKIT_SBOM_SCAN_STAGE=true

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY examples/custom-protocol ./examples/custom-protocol
COPY migrations ./migrations
COPY .sqlx ./.sqlx

RUN --mount=type=cache,id=fleetiq-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=fleetiq-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=fleetiq-target,target=/workspace/target,sharing=locked \
    SQLX_OFFLINE=true cargo build --locked --release -p fleetiq-server -p fleetiq-protocol-wasm --bins && \
    cp target/release/fleetiq-server /tmp/fleetiq-server && \
    cp target/release/fleetiq-protocol-admission /tmp/fleetiq-protocol-admission

# Generate the inventory before entering the shell-free runtime stage.
RUN /tmp/fleetiq-server migration-inventory > /tmp/migrations.json

# The Rust executable needs glibc and libgcc, not a package manager, shell or curl.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:3ee00291cf386ec52a552e6a858d555debd74776eb4bfa53fa3df2cd6677515e AS runtime

COPY --from=builder /tmp/fleetiq-server /usr/local/bin/fleetiq-server
COPY --from=builder /tmp/fleetiq-protocol-admission /usr/local/bin/fleetiq-protocol-admission
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
