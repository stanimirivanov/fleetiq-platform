# syntax=docker/dockerfile:1.7

# Compile with the repository-pinned Rust release. SQLx uses the committed
# offline query descriptions so an image build never needs database access.
FROM rust:1.99.0-slim-bookworm AS builder

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
COPY .sqlx ./.sqlx

RUN --mount=type=cache,id=fleetiq-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=fleetiq-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=fleetiq-target,target=/workspace/target,sharing=locked \
    SQLX_OFFLINE=true cargo build --locked --release -p fleetiq-server && \
    cp target/release/fleetiq-server /tmp/fleetiq-server

# Keep compilers and source outside the runtime image. curl exists only for the
# local container health check and can be reconsidered with production probes.
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install \
        --yes --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 fleetiq && \
    useradd --uid 10001 --gid fleetiq --no-create-home \
        --shell /usr/sbin/nologin fleetiq

COPY --from=builder /tmp/fleetiq-server /usr/local/bin/fleetiq-server

ENV FLEETIQ_BIND_ADDRESS=0.0.0.0:8080

EXPOSE 8080

USER 10001:10001

HEALTHCHECK --interval=5s --timeout=3s --start-period=10s --retries=10 \
    CMD ["curl", "--fail", "--silent", "--show-error", "--output", "/dev/null", "http://127.0.0.1:8080/health/ready"]

ENTRYPOINT ["/usr/local/bin/fleetiq-server"]
